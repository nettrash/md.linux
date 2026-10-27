# md for Linux — the real-desktop checklist

Everything the shell's self-test cannot prove, gathered from every work package's "not verified
here" list (docs/shell-work-packages.md §4–§5), shell-design §12, §13.4 and §16, linux-facts §9–§10
and the integration gate, deduplicated and **ordered by risk**: what would stop md launching first,
then what a user would meet on the first day. The self-test runs on X11 under Xvfb in Docker, with
WebKit's sandbox off, no window manager, no input method and no printer; everything below depends
on one of those.

Run it on a fresh **Ubuntu 26.04 desktop, GNOME on Wayland**, once on **amd64** and once on
**arm64** (a VM is fine), as an ordinary user, with the `.deb` from `scripts/dev.sh package`
installed by docs/packaging.md §11 (its install steps 1–11 come first; this list is its step 12).
Write the build's SHA-256 at the top of the run. Each line: **how to check** → **what correct looks
like**. A line that fails names its section in shell-design for whoever fixes it.

Sample files: make `~/mdtest/` with `a.md` (a heading, a list, a table, a `$x^2$`, a ```` ```mermaid
```` block and `![](https://nettrash.me/favicon.ico)`), `long.md` (2000 numbered lines), `a.mdown`,
`a.text`, `a.puml`, `a.gv`, a `.textpack` exported from md itself, and a copy of the Example Book
(File ▸ Examples ▸ Example Book…).

---

## 1. Launch blockers — the WebKit sandbox under AppArmor (§4.1, packaging §7)

1. `sudo aa-status | grep -w md` and `cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns`
   → the `md` profile is listed (unconfined mode) and the sysctl is `1`: the restriction is on and
   md is exempted by its profile.
2. Launch md from the app grid, open `a.md` → the preview renders (math, Mermaid). No alert "The
   Preview Cannot Start"; md does not vanish.
3. `pgrep -a WebKitWebProcess`, then `grep NSpid /proc/<pid>/status` → two numbers (the web process
   runs in its own PID namespace: bubblewrap is on). `env` of md's process (`tr '\0' '\n' <
   /proc/$(pgrep -x md)/environ`) has **no** `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS`.
4. The preflight's failure path: `sudo apparmor_parser -R /etc/apparmor.d/md`, launch md → one
   alert, "The Preview Cannot Start", naming `/etc/apparmor.d/md`; OK quits md cleanly (no core
   dump in `journalctl -b | grep -i md`). `sudo apparmor_parser -r /etc/apparmor.d/md` and md
   starts again.
5. File ▸ Examples ▸ Diagrams and Math → Mermaid, Graphviz, PlantUML and KaTeX all draw offline
   (turn networking off for this line).

## 2. Wayland — activation, full screen, the hidden renderer (§1.2, §1.4, §5.3, §7.1, §12)

6. With md running and **another app focused**, double-click `a.md` in Files → md's window for it
   comes **to the front with focus** (xdg-activation token honoured), not a "md is ready"
   notification. Again for a file already open → the same window comes forward, no second window.
7. `md --new-window` from a terminal, and the dock icon's right-click ▸ New Document → a new
   untitled window, focused.
8. Window ▸ (another document's row) and the Book window's Handoff page ▸ **Show Window** → that
   window is raised and focused (no attention-request flash instead).
9. View ▸ Enter Full Screen (F11) and back; View ▸ Zen Mode (Shift+Ctrl+Return), Esc, and F11
   inside Zen → full screen follows every time, the View menu's row reads Enter / Exit Full Screen
   correctly, and leaving full screen by any route drops Zen (the column goes back to the panes).
   Super+Up / double-click on the header maximise and restore normally.
10. Zen geometry → one column, two thirds of the screen wide and 92 % tall, centred; the capsule
    (write / read / exit) appears at the top when the pointer moves and fades after ~2.5 s.
11. Export ▸ PDF…, EPUB… (on `a.md`, which has math and a diagram) and Diagram as SVG → the files
    are written and correct; **nothing flashes on screen** while they render (the renderer is an
    invisible overlay that must stay mapped on Wayland's GL/Vulkan renderer). Open the EPUB in
    Foliate: the formula and the diagram are crisp images, not blank boxes.
12. GPU / DMA-BUF: on the machine's own GPU (Intel or AMD; NVIDIA if there is one) the preview
    paints, scrolls and resizes without black or white rectangles. If NVIDIA shows a blank page,
    note whether `WEBKIT_DISABLE_DMABUF_RENDERER=1 md` cures it (a user workaround; md never sets
    it).
13. Fractional scaling: Settings ▸ Displays ▸ 125 % (and 200 %) → editor text and menus crisp;
    re-export the EPUB of line 11 → its formula images are sharp at the new scale (the 2× snapshot
    divides by the surface scale, §7.5).
14. Clipboard: select in the editor, Ctrl+C, paste into gedit / Text Editor; select in the
    preview, Ctrl+C, paste → both work. Select text in the editor, then select in another app →
    md's selection stays visible (the X11 PRIMARY workaround in `EditorPane::select_in` must do no
    harm on Wayland).

## 3. Input methods and keyboard layouts (§2.6, §3.3, §16.7)

15. US-international (or any dead-key layout), at the start of a line: `'` then `e` → **É** (the
    dead key's commit is capitalised like typing). Ctrl+Z → `é`.
16. An IBus CJK engine (Pinyin or Anthy): compose a word at a sentence start and commit → the text
    is committed as composed, **no capital and no list edit** applied to it; the candidate window
    sits at the caret.
17. Select a word, type `*` → `*word*` with `word` selected; **Ctrl+Z once** → `*`; **once more**
    → `word`, selected (the two-step undo the Mac has). Do the same with an IBus engine active
    (Latin mode): same result.
18. German layout: AltGr+2 types `²`; left Ctrl+Alt+2 makes Heading 2. On a layout where `[` `]`
    need AltGr (German, French): Ctrl+AltGr+8/9 or the physical Ctrl+[ / Ctrl+] keys → Outdent /
    Indent (note which works).
19. Typing speed: hold a letter key in `long.md` → no lag, no dropped characters; type a sentence
    quickly with capitals → each capital is applied and Ctrl+Z after one restores lowercase.

## 4. GNOME Shell's shortcuts (§2.8, §2.9, §16.9)

20. In the Book window: **Alt+Down / Alt+Up** step articles; Ctrl+Alt+Up/Down switch workspaces
    (GNOME's), never articles — the tooltips say Alt+Up / Alt+Down.
21. F10 opens the menu bar; F1 opens the support page in the browser; Ctrl+Q, Ctrl+W, Ctrl+N,
    Ctrl+O, Shift+Ctrl+S, Ctrl+P, Ctrl+1/2/3, Shift+Ctrl+B, Shift+Ctrl+Return, F11 each do their
    row; none is eaten by the Shell.
22. In the find field: Ctrl+Z undoes the field, Ctrl+A selects the field's text; in the preview:
    Ctrl+C copies its selection; in Preview mode Ctrl+Z does nothing.

## 5. Printing (§7.2, §16.5)

23. File ▸ Print… on `a.md` → GTK's print dialog over A4, 0.5 in margins; print to a real
    printer or CUPS-PDF (`sudo apt install printer-driver-cups-pdf`) → white paper, dark ink,
    Nimbus Mono PS prose, the math and the diagram printed. Cancel instead → no hang, the window
    still closes normally afterwards.
24. Book ▸ Print Book… from the Book window and from a document window with the Book window
    closed → the same dialog; the whole book prints in reading order.
25. Close a window while its Export ▸ PDF… is still rendering (a big book PDF) → the window waits
    (at most 10 s), the file is written, then the window goes; md does not crash.

## 6. Files, the desktop entry and MIME (packaging §2–§4, §11)

26. Files: right-click `a.mdown`, `a.text`, `a.puml`, `a.gv`, the `.textpack` ▸ Open With → md is
    listed; opening each works (the `.textpack` as an untitled window titled after it).
27. Files ▸ Recent → the documents md opened are listed; md's File ▸ Open Recent shows the ten
    newest, and **Clear Menu** empties md's list while Files' Recent keeps them.
28. Drag `a.md` from Files onto the editor and onto the preview of an open window → it opens in
    its own window; drag a `.textbundle` folder → it imports; drag selected text from gedit into
    the editor → the text is inserted, no window opens.
29. The dock: md's icon (not a generic one) at every zoom; md's windows group under it; the app
    grid finds it by "markdown". Help ▸ About md shows the same icon (the dev image has none
    installed, so the self-test's screenshot shows GTK's missing-icon picture), "nettrash",
    1.6.0, and Details lists the five bundled engines.

## 7. The file chooser (§6.1, PRIVACY)

30. File ▸ Open… → the desktop's chooser (likely the portal's): **no Encoding row**, one filter
    named "Supported Documents"; a CP1251 or UTF-16 file opens with the right characters.
31. Save As… on an untitled window, type `notes` with no extension → `notes.md` is written; type
    `notes` again where `notes.md` exists → md asks "“notes.md” already exists. Do you want to
    replace it?". Save As… on a saved document starts in its folder.
32. `gsettings list-recursively org.gtk.gtk4.Settings.FileChooser` after a few dialogs → only the
    chooser's own preferences (size, sort, hidden files), no file names — what PRIVACY.md says
    md's dialogs leave in `~/.config/dconf/user`.

## 8. Logout, quit and the session (§1.5, §1.6)

33. Type in an untitled window (do not save) and in a saved one, then **log out** → no dialog
    blocks the logout; log back in, launch md → the draft returns marked "— Edited" with its text,
    and the saved document with its layout and window size. (`query-end`.)
34. Same with Ctrl+Q instead of logout → same result. Close the untitled window with its close
    button instead → "Do you want to save the changes…?" appears; Don't Save deletes the draft
    (`ls ~/.local/share/md/drafts` empty).
35. Window sizes are restored; positions are left to GNOME (windows do not reopen where they were
    — expected, §1.4).

## 9. Light and dark (§4.8, §10)

36. Settings ▸ Appearance ▸ Dark while a document with Mermaid and PlantUML is open → window,
    menus, dialogs and preview turn to carbon paper **without a white flash**; the diagrams redraw
    dark (the first Mermaid block may stay light — the family-wide trait, §16.11). Back to Light.
37. A non-default Yaru accent colour in Settings → md keeps its own brown accent (toggles, caret,
    selection); the menu bar row and header read well in both themes.

## 10. Typography (§10, §16.8)

38. Nimbus Mono PS at 15 pt in the editor and the preview on the real display → legible, not
    spindly; the caret is the accent colour; Cyrillic and Greek render in the same face. Note
    whether Courier Prime would be needed (the open question).
39. CJK text in a document → shows in a CJK font if one is installed (the dev image had none), no
    tofu.

## 11. The network (PRIVACY, §4.10, §7.5)

40. `a.md`'s remote image → shown in the preview, in Print and in PDF. Offline → an empty box,
    nothing else breaks.
41. A document with front matter `cover: https://…/cover.jpg` → Export ▸ EPUB… puts the cover on
    the book (Foliate's library shows it). A redirecting URL to `http:` → no cover, no error.
42. Clicking an `https:` link in the preview → opens in the browser; a `[x](other.md)` link → does
    nothing.

## 12. Removable and network folders (§6.7, §8.2)

43. Open a document on an SMB or NFS mount, edit it from another machine → no reload (no events
    there), but md's next autosave refuses to clobber it and the conflict bar appears.
44. Open a book on a USB drive, unplug it → the Book window shows No Book Open (not
    accessible), the setting is kept; plug it in and Book ▸ Open Book… / relaunch → it comes back.

## 13. Feel (§5, §8)

45. Book sidebar: drag its edge → it resizes between 200 and 320 px and no further; View ▸ Show
    Sidebar hides and restores it. Full screen (F11) in the Book window works as a writing room.
46. Split on `long.md`: touchpad-scroll the editor → the preview follows; scroll the preview → the
    editor follows; neither jitters. Pinch on the preview → it does not zoom.
47. Narrow a Split window below 640 px of pane width → the panes stack; widen → side by side.

## 14. Remove and purge (packaging §11 items 13–14)

48. `sudo apt remove md`, then `sudo apt purge md` → as packaging §11 describes: the profile stays
    after remove and goes with purge; `~/.config/md`, `~/.local/state/md`, `~/.local/share/md`
    stay in both.
