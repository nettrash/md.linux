# Privacy Policy

**Effective date:** 27 October 2026
**Applies to:** md for Linux — the Ubuntu Markdown editor published by
nettrash as a `.deb` package. This policy is versioned alongside the app's
source code; the most recent commit on `main` is authoritative.

## TL;DR

md **does not collect, transmit, sell, or share any data.** It contains
no analytics, no advertising SDKs, no third-party trackers, and no
servers operated by us. The documents you open and edit stay where you
put them — on your computer, or in folders your own cloud provider syncs.

If that already answers your question, you don't need to read the rest.

## What we collect

**Nothing.** md has no account to create, no email to register, and no
telemetry. It contacts no servers of ours — there are none — and it does not
check for updates; new versions arrive as a new `.deb`. The one time the app
touches the network at all is when a document you opened points at an image
by remote URL, and the renderer fetches that image so it can be shown,
printed and exported — or, when you export an EPUB, its front matter names
its cover by an `https:` URL (see **Permissions** below).

## Your documents

md is a document editor. The files you open, create and save are handled
through GTK's file dialogs (on GNOME, the desktop's own file chooser), file
associations and drag and drop, and are stored wherever you choose — locally
or in a folder your own cloud provider syncs. We never see them. If you keep
a document in a synced folder, it syncs through *your* account under *that*
provider's privacy terms, not ours.

The app stores a few small settings in your home folder, in
`~/.config/md/settings.conf` (or under `$XDG_CONFIG_HOME`). They are, in
full:

- `md.viewModeMemory` — which layout (Edit / Split / Preview) you last had
  each of your 200 most recently opened documents in. Each entry is a short
  hash of the file's path, not the path itself, and a mode.
- `md.pdfPageSize` — the page size chosen for PDF export (A4 unless you
  pick another).
- `md.bookBookmark` — the path of the book folder you have open in writer
  mode, if any. **Book ▸ Close Book** removes it.
- `md.bookLastArticle` — which article in that book you had open last, as a
  path relative to the book, so the book reopens where you left it.
- `md.bookOpensInSeparateWindows` — whether a book opens its articles in
  separate windows.
- `md.bookViewMode` — the book window's one layout.
- `md.linux.windowSize.document` and `md.linux.windowSize.book` — the last
  size of a document window and of the book window, so new windows open at
  it.
- `md.continueLists`, `md.capitalizeSentences` and `md.wrapSelection` — the
  three switches under **Edit ▸ Typing**, on or off. All three start on.
- `md.linux.recentClearedAt` — when you last chose **File ▸ Open Recent ▸
  Clear Menu**, so md's menu shows only what you opened after it.

Beside those settings the app keeps exactly the following:

- `~/.local/state/md/session.json` (or under `$XDG_STATE_HOME`) — the
  documents and drafts you had open when md last closed, with each one's
  layout, Zen state, window size and whether it was maximized, and whether
  the book window was open and its size, so the next launch reopens them. It
  holds file paths and window sizes, nothing else.
- `~/.local/share/md/drafts/` (or under `$XDG_DATA_HOME`) — the text of
  untitled documents you have not saved yet, one file each, readable only
  by you. A draft is deleted when you save the document or close its window
  with **Don't Save**.
- **Temporary PDF files** in `$XDG_RUNTIME_DIR/md/` — the pages an export or
  a book compile renders before they are copied to the file you named; each
  is deleted as soon as it has been copied, and the folder is in memory and
  emptied when you log out (without a runtime folder, the system's temporary
  folder is used instead).

The preview keeps **no web data**: it runs with an in-memory session, so no
cookies, cache or browsing state is written anywhere. The per-document
encoding, line endings and scroll position live in memory only.

Two things are held for md by the desktop itself: the **recent documents
list** in `~/.local/share/recently-used.xbel`, which md adds a document to
when you open or save it, and which GNOME Files and other applications share
(**Clear Menu** empties md's menu without deleting anybody else's entries;
md removes an entry whose file no longer exists); and whatever GTK's file
chooser remembers about its own use, such as its last size.

One file may be written beside *your* document rather than in md's folders:
if a save fails while a window is closing — a full disk, a read-only folder —
md keeps your text next to the original as `<name> (rescued)` with the
original extension (then `(rescued 2)`, and so on) — and tells you so. It is
your document, in your folder, and md never touches it again.

None of these settings and files leave your computer, and none of them
contain personal information beyond the file paths named above. Removing or
purging the package leaves them in place, as Debian packages always leave
the files in your home folder; delete the folders above to remove them. If a
future version remembers anything else, it is added to this list in the
same commit that adds the setting.

## Permissions

md asks for no permissions and none are needed: like any program you install
on Linux, it runs with your own user's rights. That is worth stating plainly,
because it is not a sandbox. md reads and writes only the files you open,
save, drop on it or pick as a book — by how it is written, which anyone can
read in its public source, not by a fence the operating system draws around
it.

The preview is different: WebKitGTK runs its web content in processes of
their own, inside a sandbox (bubblewrap) that needs an unprivileged user
namespace. Ubuntu 26.04 grants one only to programs an AppArmor profile
names, so the package installs `/etc/apparmor.d/md`. That profile confines
nothing and grants nothing else; it is the same profile Ubuntu ships for its
own WebKitGTK applications, and it exists only so that the preview's sandbox
can start.

md does use the network for one thing. If a document you open references an
image by remote URL (`![alt](https://…)`), the renderer fetches that image so
it can be shown, printed and exported — that request goes straight to the
host **your own document names**, which sees your IP address exactly as it
would if you opened the link in a browser. It happens only for documents that
contain such a link. The same goes for an EPUB export whose front matter says
`cover: https://…`: the export fetches that one image to put it on the book,
and nothing else. If that host redirects the request, md follows at most five
redirects, and only to `https` addresses; each host along the way sees your
IP address in the same way. A local image beside the document is deliberately
*not* shown in the preview (the family decision: md's document is its text);
the one time md reads a file beside your document is **Export ▸
TextBundle…**, which copies the local images the document links into the new
bundle's `assets/` — from disk, at your request — and, likewise, **Export ▸
EPUB…** reads the one image a `cover:` in the front matter names, from the
document's own folder only.

A link you click in the preview, **Help ▸ md Help** and **Help ▸ Privacy
Policy** open in your web browser; md itself fetches nothing for them.

Everything else is built on your computer: the Markdown renderer, and the
math and diagram engines (KaTeX with mhchem, Mermaid, Graphviz, PlantUML,
highlight.js) are installed with the app and run offline. No code is
downloaded or updated at run time. md sends no data anywhere, and contacts
no servers of ours, because there are none.

## Children's privacy

Because md collects no data at all, it collects no data from children.

## Changes to this policy

Any change is committed to this file in the app's public source
repository, so the history is auditable.

## Contact

Questions: <nettrash@nettrash.me>.
