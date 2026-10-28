# md for Linux — the .rpm for Fedora 44 and later (docs/packaging.md §12).
#
# Built by scripts/package-rpm.sh from a staged tree of an already compiled, stripped binary and
# its data (md_stage), so there is no %build; md_version comes from Cargo.toml. The files are the
# .deb's (§1) without what only Debian and Ubuntu use: the AppArmor profile (Fedora confines with
# SELinux and lets WebKit's sandbox make its user namespace) and lintian's overrides. The MIME,
# desktop and icon caches are refreshed by the file triggers of shared-mime-info,
# desktop-file-utils and hicolor-icon-theme, so there are no scriptlets.

%global debug_package %{nil}

Name:           md
Version:        %{md_version}
Release:        1%{?dist}
Summary:        Markdown editor and live previewer
# md and the crates linked into it: MIT. The bundled preview engines (usr/share/md/rich; the
# copyright file names each): KaTeX MIT, its fonts OFL-1.1; mhchem Apache-2.0 AND MIT; Mermaid MIT
# AND ISC AND BSD-3-Clause AND (Apache-2.0 OR MPL-2.0); Viz.js MIT AND EPL-1.0; highlight.js
# BSD-3-Clause; PlantUML GPL-3.0-or-later AND EPL-1.0 AND Apache-2.0.
License:        MIT AND OFL-1.1 AND Apache-2.0 AND ISC AND BSD-3-Clause AND (Apache-2.0 OR MPL-2.0) AND EPL-1.0 AND GPL-3.0-or-later
URL:            https://nettrash.me/linux/md/support.html

Requires:       hicolor-icon-theme
# The editor's and the paper's face, and the code's: by the font capability Fedora's font packages
# provide (urw-base35-nimbus-mono-ps-fonts, liberation-mono-fonts).
Requires:       font(nimbusmonops)
Requires:       font(liberationmono)

%description
md is a Markdown editor with a live preview beside the text: Edit, Split and
Preview layouts, Zen mode, find and replace, continued lists and capitalized
sentences, and a Format menu.

The preview draws KaTeX math and chemistry, Mermaid, Graphviz and
PlantUML diagrams and highlighted code with bundled open source engines, and
plots with none. Documents export to PDF, HTML, EPUB, LaTeX and TextBundle; a
folder of Markdown files can be written and compiled as a book.

%install
cp -a %{md_stage}/. %{buildroot}/

%files
%dir %{_datadir}/licenses/%{name}
%license %{_datadir}/licenses/%{name}/copyright
%dir %{_docdir}/%{name}
%doc %{_docdir}/%{name}/README.md
%doc %{_docdir}/%{name}/CHANGELOG.md
%doc %{_docdir}/%{name}/PRIVACY.md
%{_bindir}/md
%{_datadir}/md/
%{_datadir}/applications/me.nettrash.md.desktop
%{_datadir}/icons/hicolor/*/apps/me.nettrash.md.png
%{_datadir}/metainfo/me.nettrash.md.metainfo.xml
%{_datadir}/mime/packages/me.nettrash.md.xml
%{_mandir}/man1/md.1*

%changelog
* Tue Oct 27 2026 nettrash <nettrash@nettrash.me> - 1.6.0-1
- First release for Linux: md for Fedora 44 and later, x86_64 and aarch64,
  with the functionality of md for macOS 1.6. The user-facing history is
  CHANGELOG.md beside the README.
