//! Port of md.win `Export/RenderCompletePoller.cs`: the render-complete handshake (§4.8, export.md
//! §3.3), the Mac's loop line for line. Read `data-md-render-complete`, and if it is not yet the
//! flag, wait 250 ms and read it again, up to [`RenderCompletePoller::MAX_ATTEMPTS`] more times.
//! **The timeout is a success, not a failure** — after about two minutes the export proceeds with
//! whatever the DOM holds, because a diagram that never arrived has already restored its own
//! source text, and throwing a whole export away over one figure is worse than exporting it as
//! text. Only a failed navigation aborts, and that is the surface's business, not this loop's.
//!
//! The comparison is against the three characters `"1"`, quotes included: the evaluation hands back
//! the JSON *encoding* of the result and the flag is a string attribute, which is exactly what
//! md.Android compares. Anything else — `null` from a dead page, a number, whitespace — is "not
//! yet".
//!
//! The live preview never waits: it renders while you type and a half-drawn diagram is the point.
//! This is exports only.
//!
//! On Linux the app's [`RenderSurface::load`] awaits WebKit's `LoadEvent::Finished` (a failed load
//! is `RenderError::Failed(PAGE_NOT_CAPTURED)`) and then calls [`RenderCompletePoller::wait`] on
//! itself. md-init.js's `mdRender` script message arrives on WebKitGTK as well
//! (docs/linux-facts.md §4) and may end the wait early, but the poll is the contract: it is what
//! makes a page that never posts still finish after two minutes.

use std::rc::Rc;
use std::time::Duration;

use super::export_delay;
use crate::exec::CancelToken;
use crate::preview::scripts;
use crate::seams::{RenderError, RenderSurface, Scheduler};

/// The render-complete wait over one surface's [`RenderSurface::eval`].
pub struct RenderCompletePoller {
    scheduler: Rc<dyn Scheduler>,
}

impl std::fmt::Debug for RenderCompletePoller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderCompletePoller").finish_non_exhaustive()
    }
}

impl RenderCompletePoller {
    /// Retries after the first read: 480 × 250 ms ≈ 120 s, the number all four ports use.
    pub const MAX_ATTEMPTS: usize = 480;

    /// The gap between reads.
    pub const INTERVAL: Duration = Duration::from_millis(250);

    pub fn new(scheduler: Rc<dyn Scheduler>) -> Self {
        RenderCompletePoller { scheduler }
    }

    /// Returns when the page says it has finished rendering, or when the attempts run out — both
    /// are `Ok`. `surface` is the loading surface itself. An evaluation that fails is the surface's
    /// error, returned as it is; a cancelled `cancel` is [`RenderError::Cancelled`], noticed before
    /// every read and when a wait ends (see [`export_delay`] for when that is).
    pub async fn wait(&self, surface: &dyn RenderSurface, cancel: &CancelToken) -> Result<(), RenderError> {
        let mut attempt = 0;
        loop {
            if cancel.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            let json = surface.eval(scripts::RENDER_COMPLETE.to_string()).await?;
            if json == scripts::RENDER_COMPLETE_RESULT || attempt >= Self::MAX_ATTEMPTS {
                return Ok(());
            }
            export_delay::delay(&*self.scheduler, Self::INTERVAL, cancel).await?;
            attempt += 1;
        }
    }
}
