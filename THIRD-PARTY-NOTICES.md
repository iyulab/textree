# Third-Party Notices

Textree is distributed under the GPL-3.0-only license (see `LICENSE`). The binary
distribution also contains third-party components, each under its own license.

**This file is incomplete.** It currently covers the embedded git engine and the native
libraries compiled into it, because those carry terms that constrain how the whole
distribution may be combined and shipped. The remaining dependency tree — the Rust crates
this application is built from, and their transitive dependencies — is predominantly
MIT and Apache-2.0 and is not yet enumerated here. Those licenses do require their
copyright notices to accompany a binary distribution, so this file will be completed.

Every component below is listed with the exact version compiled into the release.

## libgit2

- Version: 1.9.6, vendored via `libgit2-sys 0.18.7+1.9.6`, statically linked, unmodified
- Upstream: https://github.com/libgit2/libgit2
- License: **GPL-2.0-only WITH a linking exception**

libgit2 is licensed under version 2 of the GNU General Public License with the following
exception, quoted from its `COPYING` file:

> In addition to the permissions in the GNU General Public License, the authors give you
> unlimited permission to link the compiled version of this library into combinations with
> other programs, and to distribute those combinations without any restriction coming from
> the use of this file. (The General Public License restrictions do apply in other respects;
> for example, they cover modification of the file, and distribution when not linked into a
> combined executable.)

libgit2 is not relicensed by its inclusion here; it remains under the terms above. It is
compiled with a minimal feature set — no TLS backend and no SSH transport — so no OpenSSL or
libssh2 code is present in this distribution.

### Components bundled inside libgit2

| Component | License | Upstream |
| --- | --- | --- |
| xdiff | LGPL-2.1-or-later | https://github.com/libgit2/libgit2/tree/main/deps/xdiff |
| llhttp | MIT | https://github.com/nodejs/llhttp |
| PCRE2 | BSD-3-Clause | https://github.com/PCRE2Project/pcre2 |

## Rust crates wrapping the git engine

| Crate | License | Upstream |
| --- | --- | --- |
| `git2`, `libgit2-sys` | MIT OR Apache-2.0 | https://github.com/rust-lang/git2-rs |
| `libz-sys` (zlib) | zlib | https://zlib.net |

The full license text for each component is available at the upstream location listed above.
