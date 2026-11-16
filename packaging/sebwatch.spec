# Binary package from a built checkout: scripts/package rpm builds the release binary first and
# passes the version from the workspace Cargo.toml and the checkout as the source folder.
%global debug_package %{nil}

Name:           sebwatch
Version:        %{pkg_version}
Release:        1
Summary:        See what each app uses: CPU, memory, GPU and disk
License:        MIT
URL:            https://github.com/S-Sebb/SebWatch
ExclusiveArch:  x86_64

# Loaded at run time, so rpm's ELF scan doesn't find them: the windowing system (Wayland or X11),
# keyboard layouts and GL through EGL or GLX; fontconfig is linked and found. NVML comes with the NVIDIA driver and is
# optional.
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-cursor.so.0()(64bit)
Requires:       libwayland-egl.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libX11.so.6()(64bit)
Requires:       libX11-xcb.so.1()(64bit)
Requires:       libXcursor.so.1()(64bit)
Requires:       libXi.so.6()(64bit)
Requires:       libXrandr.so.2()(64bit)
Requires:       libEGL.so.1()(64bit)
Requires:       libGL.so.1()(64bit)
Requires:       hicolor-icon-theme

%description
SebWatch shows how much RAM, swap, CPU, disk, GPU and VRAM each app uses, with the parts adding
up to the totals.

%install
cd %{_sourcedir}
install -Dm755 target/release/sebwatch %{buildroot}%{_bindir}/sebwatch
install -Dm644 packaging/sebwatch.desktop %{buildroot}%{_datadir}/applications/sebwatch.desktop
for icon in art/icons/hicolor/*/apps/sebwatch.png; do
    install -Dm644 "$icon" "%{buildroot}%{_datadir}/icons/${icon#art/icons/}"
done
install -Dm644 LICENSE %{buildroot}%{_datadir}/licenses/%{name}/LICENSE

%files
%dir %{_datadir}/licenses/%{name}
%license %{_datadir}/licenses/%{name}/LICENSE
%{_bindir}/sebwatch
%{_datadir}/applications/sebwatch.desktop
%{_datadir}/icons/hicolor/*/apps/sebwatch.png
