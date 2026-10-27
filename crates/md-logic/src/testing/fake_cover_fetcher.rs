//! A fake for the [`CoverFetcher`] seam — md.win's tests replace the whole `fetchCover` function
//! instead (`ExportHarness.Fetch`); here the seam is one request, so the fake answers requests.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use super::answer_queue::AnswerQueue;
use crate::exec::{Completion, LocalBoxFuture, ready};
use crate::export::{CoverBody, CoverFetcher, CoverResponse};

/// One scripted response: status, `Location`, declared length and the body's chunks. A chunk of
/// `Err` fails the transfer there; [`FakeCoverAnswer::hold_body`] leaves the body waiting after its
/// chunks (a server that stalls).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeCoverAnswer {
    pub status: u16,
    pub location: Option<String>,
    pub content_length: Option<u64>,
    pub chunks: Vec<Result<Vec<u8>, String>>,
    pub hold_body: bool,
}

impl FakeCoverAnswer {
    /// 200 with `body` as one chunk and its length declared.
    pub fn ok(body: &[u8]) -> Self {
        FakeCoverAnswer {
            status: 200,
            location: None,
            content_length: Some(body.len() as u64),
            chunks: vec![Ok(body.to_vec())],
            hold_body: false,
        }
    }

    /// 200 with the body in these chunks and no declared length (a chunked transfer).
    pub fn chunked(chunks: &[&[u8]]) -> Self {
        FakeCoverAnswer {
            status: 200,
            location: None,
            content_length: None,
            chunks: chunks.iter().map(|chunk| Ok(chunk.to_vec())).collect(),
            hold_body: false,
        }
    }

    /// A redirect (`status` 3xx) to `location`, or with no `Location` at all.
    pub fn redirect(status: u16, location: Option<&str>) -> Self {
        FakeCoverAnswer {
            status,
            location: location.map(str::to_string),
            content_length: Some(0),
            chunks: Vec::new(),
            hold_body: false,
        }
    }

    /// `status` with an empty body.
    pub fn status(status: u16) -> Self {
        FakeCoverAnswer { status, location: None, content_length: Some(0), chunks: Vec::new(), hold_body: false }
    }

    /// The same answer with another declared length.
    pub fn with_content_length(mut self, length: Option<u64>) -> Self {
        self.content_length = length;
        self
    }

    /// The same answer whose body stalls after its chunks.
    pub fn stalling(mut self) -> Self {
        self.hold_body = true;
        self
    }
}

/// [`CoverFetcher`] with scripted answers: queued ones first (FIFO, [`FakeCoverFetcher::queue`],
/// [`FakeCoverFetcher::hold`]), then [`FakeCoverFetcher::set_handler`]'s, then `Err("offline")`.
/// Every request is recorded ([`FakeCoverFetcher::requests`]), and so is every chunk handed out
/// ([`FakeCoverFetcher::chunks_read`]) — how a test proves the cap stopped the read.
#[derive(Default)]
pub struct FakeCoverFetcher {
    requests: RefCell<Vec<String>>,
    answers: AnswerQueue<Result<FakeCoverAnswer, String>>,
    handler: RefCell<Option<Rc<dyn Fn(&str) -> Result<FakeCoverAnswer, String>>>>,
    chunks_read: Rc<Cell<usize>>,
}

impl fmt::Debug for FakeCoverFetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeCoverFetcher").field("requests", &self.requests.borrow()).finish()
    }
}

impl FakeCoverFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every URL requested, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.borrow().clone()
    }

    /// How many body chunks were handed out, over every response.
    pub fn chunks_read(&self) -> usize {
        self.chunks_read.get()
    }

    /// The next request is answered with `answer`.
    pub fn queue(&self, answer: Result<FakeCoverAnswer, String>) {
        self.answers.push(answer);
    }

    /// The next request waits for the completion (a server that never answers).
    pub fn hold(&self) -> Completion<Result<FakeCoverAnswer, String>> {
        self.answers.hold()
    }

    /// Every request with no queued answer is answered by `handler`.
    pub fn set_handler(&self, handler: impl Fn(&str) -> Result<FakeCoverAnswer, String> + 'static) {
        *self.handler.borrow_mut() = Some(Rc::new(handler));
    }
}

impl CoverFetcher for FakeCoverFetcher {
    fn get(&self, url: String) -> LocalBoxFuture<'static, Result<CoverResponse, String>> {
        self.requests.borrow_mut().push(url.clone());
        let handler = self.handler.borrow().clone();
        let answer = self.answers.next_or(move || match handler {
            Some(handler) => handler(&url),
            None => Err("offline".to_string()),
        });
        let chunks_read = self.chunks_read.clone();
        Box::pin(async move {
            let answer = answer.await?;
            Ok(CoverResponse {
                status: answer.status,
                location: answer.location,
                content_length: answer.content_length,
                body: Box::new(FakeCoverBody {
                    chunks: RefCell::new(answer.chunks.into()),
                    hold: answer.hold_body,
                    chunks_read,
                }),
            })
        })
    }
}

struct FakeCoverBody {
    chunks: RefCell<VecDeque<Result<Vec<u8>, String>>>,
    hold: bool,
    chunks_read: Rc<Cell<usize>>,
}

impl CoverBody for FakeCoverBody {
    fn next_chunk(&self) -> LocalBoxFuture<'static, Result<Option<Vec<u8>>, String>> {
        match self.chunks.borrow_mut().pop_front() {
            Some(chunk) => {
                self.chunks_read.set(self.chunks_read.get() + 1);
                ready(chunk.map(Some))
            }
            // A stalled body never answers; nothing completes it.
            None if self.hold => Completion::new().wait(),
            None => ready(Ok(None)),
        }
    }
}
