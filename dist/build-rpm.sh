#!/bin/bash
set -euo pipefail

NAME="arclight"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
RELEASE="${RELEASE:-1}"
CHANGELOG_MSG="${1:-$(git log -1 --format=%s)}"
TOPDIR="$(pwd)/rpmbuild"

echo "> Cleaning previous build..."
rm -rf "$TOPDIR"
mkdir -p "$TOPDIR"/{SOURCES,SPECS,BUILD,RPMS,SRPMS}

echo "> Creating source tarball..."
tar czf "$TOPDIR/SOURCES/${NAME}-${VERSION}.tar.gz" \
    --transform="s,^,${NAME}-${VERSION}/," \
    --exclude=target --exclude=rpmbuild --exclude=.git \
    -C "$(pwd)" .

echo "> Writing spec file..."
cat > "$TOPDIR/SPECS/${NAME}.spec" << 'SPEC'
Name:           arclight
Version:        @VERSION@
Release:        @RELEASE@%{?dist}
Summary:        Native Linux launcher for World of Warcraft
License:        MIT
URL:            https://github.com/theswiftfox/arclight
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  gcc
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(openssl)

%description
Arclight is a native Linux launcher for World of Warcraft that manages
Wine/Proton prefixes, Battle.net integration, and game launching.

%prep
%autosetup

%build
cargo build --release

%install
make install DESTDIR=%{buildroot} PREFIX=%{_prefix}

%post
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

%postun
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

%files
%{_bindir}/arclight
%{_datadir}/applications/arclight.desktop
%{_datadir}/icons/hicolor/scalable/apps/arclight.svg
%{_datadir}/icons/hicolor/16x16/apps/arclight.png
%{_datadir}/icons/hicolor/24x24/apps/arclight.png
%{_datadir}/icons/hicolor/32x32/apps/arclight.png
%{_datadir}/icons/hicolor/48x48/apps/arclight.png
%{_datadir}/icons/hicolor/64x64/apps/arclight.png
%{_datadir}/icons/hicolor/128x128/apps/arclight.png
%{_datadir}/icons/hicolor/256x256/apps/arclight.png

%changelog
SPEC

sed -i "s/@VERSION@/${VERSION}/; s/@RELEASE@/${RELEASE}/" "$TOPDIR/SPECS/${NAME}.spec"

# Append changelog with dynamic date/name (outside heredoc to allow expansion)
cat >> "$TOPDIR/SPECS/${NAME}.spec" << EOF
* $(date '+%a %b %d %Y') $(git config user.name) <$(git config user.email)> - ${VERSION}-${RELEASE}
- ${CHANGELOG_MSG}
EOF

echo "> Building RPM..."
rpmbuild --define "_topdir $TOPDIR" -ba "$TOPDIR/SPECS/${NAME}.spec"

echo ""
echo "> Done! RPM(s):"
find "$TOPDIR/RPMS" -name '*.rpm'
