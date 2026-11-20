%global debug_package %{nil}

Name:           larva-php
Version:        1.13.5
Release:        1%{?dist}
Summary:        A modern PHP application server written in Rust

License:        MIT
URL:            https://github.com/larva-php/larva-php
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rustc
BuildRequires:  php-devel
Requires:       php-cgi

%description
LarvaPHP embeds PHP via the Zend Engine C API and serves PHP applications
through a high-performance asynchronous HTTP server built on Hyper and Tokio.
It supports multiple config formats (CLI, Apache, Nginx, Caddy), rate limiting,
hot reload, middleware, X-Sendfile, and a Mercure hub.

%prep
%setup -q

%build
cargo build --release -p larva-php --bin larva-php

%install
install -D -m 0755 target/release/larva-php %{buildroot}%{_bindir}/larva-php

%files
%{_bindir}/larva-php
%license LICENSE

%changelog
* Wed Nov 20 2026 LarvaPHP Contributors <dev@larva-php.local> - 1.13.5-1
- Initial RPM package.