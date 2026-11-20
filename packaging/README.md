# Packaging

Build instructions for distributing LarvaPHP through various package managers.

All builds produce the `larva-php` binary from `cargo build --release -p larva-php`.
The runtime requires `php-cgi` (PHP with the CGI SAPI) on the target system.

## Homebrew

```bash
brew install --build-from-source packaging/homebrew/larva-php.rb
# or, after publishing a tap:
brew install larva-php
```

## Debian / Ubuntu (.deb)

```bash
dpkg-buildpackage -us -uc -b
# produces ../larva-php_*_amd64.deb
sudo dpkg -i ../larva-php_*_amd64.deb
```

`debian/rules` builds with Cargo and installs to `/usr/bin/larva-php`.
`debian/control` declares the `php-cgi` runtime dependency.

## RPM (Fedora / RHEL / openSUSE)

```bash
rpmbuild -ba packaging/rpm/larva-php.spec
# produces ~/rpmbuild/RPMS/x86_64/larva-php-*.rpm
sudo dnf install ~/rpmbuild/RPMS/x86_64/larva-php-*.rpm
```

## Static binary (no package manager)

See `.cargo/config.toml` for static CRT linking, or build for musl:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl -p larva-php
# binary at target/x86_64-unknown-linux-musl/release/larva-php
```

## Standalone (embedded PHP app)

Build a tarball of your PHP app, embed it into the binary, and ship a single
executable. See `embed.rs` (`untar`) and `embed.rs::set_embedded_app_path`.