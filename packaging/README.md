# NuraLoumi package staging

Wave 1 packages are copy-only test bundles. They do not install into /usr and do
not register services.

Layout:

    bin/
      nuraloumi-panel
      nuraloumi-menu
      nuraloumi-probe
    config/
      nuraloumi.toml.example
    share/nuraloumi/
      qualification/
    LICENSE
    README.md
    MANIFEST.txt

The manifest records the target triple, Git commit, exact release build command,
and SHA-256 of every staged input. The tar writer fixes ordering, mtime, owner
and group metadata so identical staged inputs can be hash-compared.

Build and stage:

    cargo xtask build-release
    cargo xtask package

For SL101 cross artifacts, use armv7-unknown-linux-musleabihf only after
qualify-armv7 reports the Rust target and linker prerequisites. A successful
compiler check or ELF audit is not on-device qualification.
