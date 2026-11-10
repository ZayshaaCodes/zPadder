# Contributing to ZPadder

Thanks for helping. Bug reports, profiles that import badly, controller pictures and code are all
welcome.

## Reporting bugs

Open an issue with the bug template. The most useful things to include:

- the ZPadder version (Help menu) and Windows version;
- your controller, how it's connected (USB, Bluetooth, wireless adapter), and whether Steam
  Input, DS4Windows or another remapper is running (the Test bench warns about them);
- the log, from Help > Open log folder (`zpadder.log`). For more detail, start ZPadder with the
  environment variable `ZPADDER_LOG=debug` set, reproduce the problem, then attach the log;
- the profile, if the bug is about what a profile does.

## Building

You need Rust (the version in `rust-toolchain.toml`; rustup picks it up), the MSVC toolchain,
and CMake on `PATH` to build SDL3 from source. Visual Studio Build Tools include CMake. See the
README's "Building" section for details.

```powershell
cargo build                 # debug build of the workspace
cargo run -p zpadder        # the app
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check            # licences and advisories (cargo install cargo-deny)
```

CI runs the same checks on Windows, builds the core crates on Linux, and checks the minimum Rust
version in `Cargo.toml`.

### Running a development copy safely

ZPadder is single-instance: a second launch hands its command line to the running ZPadder. If you
also use ZPadder day to day, give your development copy its own instance slot and settings
folder, so it neither talks to nor overwrites your real one:

```powershell
$env:ZPADDER_INSTANCE = "dev"; $env:ZPADDER_DATA_DIR = "$PWD\target\dev-data"
cargo run -p zpadder
```

`zpadder-cli run` sends real keyboard and mouse input, so mind which window has focus.

## Code

- The workspace layout is in the README. The core crates (`zpadder-model`, `-engine`, `-config`,
  `-import-xpadder`) must stay free of OS-specific code; CI builds them on Linux.
- The engine is a pure state machine (`step(events, now, out)`). Prefer engine tests over manual
  testing: every press must own its release, so add a test whenever you touch timing, layers or
  reference counting.
- Profile format changes need a `format_version` migration in `zpadder-config`, unless they
  only add an optional field that older files simply lack (as `virtual_pad` did). Either way the
  schema must be regenerated
  (`cargo run -p zpadder --bin zpadder-cli -- schema schema/profile.schema.json`); a test fails
  otherwise.
- Virtual controllers (`crates/zpadder-runtime/src/virtual_pad.rs`) are tested with a fake
  driver, so the tests need no ViGEmBus. Trying the real thing needs ViGEmBus installed; mind
  that ZPadder must never read back its own virtual pads (the module's documentation says how).
- Every `unsafe` block needs a `// SAFETY:` comment saying why it is sound.
- Dependencies must have a permissive licence (see `deny.toml`). ZPadder makes no network
  connections, and network crates are banned.
- UI text is plain English in sentence case. Say what the user can do, not what the code does.

## The Xpadder importer

The importer is `crates/zpadder-import-xpadder`; [docs/xpadder-import.md](../docs/xpadder-import.md)
describes what it converts, and [docs/research.md](../docs/research.md) how the formats were
worked out.

| File | Job |
|---|---|
| `src/text.rs` | Tolerant reader: decoding, lines, sections, `key=value` |
| `src/keys.rs` | Slot tokens and the key-name table |
| `src/buttons.rs` | `ButtonMap`, presets and name inference |
| `src/image.rs` | `ImageLine` run-length decoding |
| `src/controller.rs` | Controller import and PNG encoding |
| `src/profile.rs` | Profile import: sets, selectors, sticks, conversions |

`tests/fixtures/` holds synthetic files written for the tests, byte-exact (CRLF,
Windows-1252; `.gitattributes` keeps git from touching them). Most public Xpadder files have no
licence, so don't add real ones. To check the importer against the public corpus, clone these
into one folder:

- [namwent/hyperspin-xpadder](https://github.com/namwent/hyperspin-xpadder) (167 profiles)
- [CareFudundae/Xpadder_profiles](https://github.com/CareFudundae/Xpadder_profiles), [sliekens/gw2-xpadder](https://github.com/sliekens/gw2-xpadder), [serafdev/xpadder-config](https://github.com/serafdev/xpadder-config), [hawkian/Gamepad-Mode-Action-Cam](https://github.com/hawkian/Gamepad-Mode-Action-Cam), [MatthewDarling/Xpadder-config](https://github.com/MatthewDarling/Xpadder-config), [exitcas/XpadderPS3](https://github.com/exitcas/XpadderPS3), [Novimatrem/xpadder-stuff](https://github.com/Novimatrem/xpadder-stuff), [gkytrash/xpadder-configs](https://github.com/gkytrash/xpadder-configs), [ParadiseAigo/xpadder-files](https://github.com/ParadiseAigo/xpadder-files), [vinylplz/xpadder](https://github.com/vinylplz/xpadder), [frrabelo/Xpadder](https://github.com/frrabelo/Xpadder), [ComandPromt/Pads-Config-Xpadder](https://github.com/ComandPromt/Pads-Config-Xpadder), [mobiwolf/xpadder2xml](https://github.com/mobiwolf/xpadder2xml)
- the gist [lotherk/e3ce62a2bc4ac746ad16](https://gist.github.com/lotherk/e3ce62a2bc4ac746ad16)

then run:

```powershell
$env:XPADDER_CORPUS = "C:\path\to\xpadder-corpus"
$env:XPADDER_CORPUS_REPORT = "C:\path\to\report.txt"   # optional: every warning, per file
cargo test -p zpadder-import-xpadder --test corpus -- --ignored --nocapture
```

It imports every file without panicking and checks that every profile passes validation. Of
those 239 files, all import, and 174 of the 220 profiles import without a single warning.
Every **assumption** in docs/xpadder-import.md is a candidate for checking against a real copy
of Xpadder: zone timing, `TurboRate`, `MouseSpeed`, `KeyTapTime`, the dead-zone scale and the
selector pairs.

## Controller pictures

Pictures must be freely licensed (public domain, CC0 or CC BY) and credited in
`assets/controllers/CREDITS.md`. Each comes with a `.layout.toml` placing every control.

## Licence

By contributing you agree that your contribution is dual-licensed under MIT OR Apache-2.0, like
the rest of ZPadder, without any additional terms.

## Releasing

This is the maintainers' checklist for publishing a release.

### Before the first public release

These need a person, not a script:

- [x] **Controller pictures.** Every picture in `assets/controllers/` has a confirmed source and
      licence (`CREDITS.md`). Keep it that way for new ones.
- [x] **Repository files.** `repository` is set in `Cargo.toml` (`ZayshaaCodes/zPadder`; the README
      badges, CHANGELOG links and the Help menu use it). README, licences, CHANGELOG,
      CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, issue and pull request templates, CI, the release
      workflow and Dependabot are in place.
- [ ] **Commit identity.** Commits are public. Make the first one with your GitHub "noreply"
      address (GitHub > Settings > Emails > "Keep my email addresses private" shows it) unless you
      want your own address published: `git config user.email "<id>+<name>@users.noreply.github.com"`.
- [ ] **Create the repository and push**, after the first commit and `gh auth login`:

  ```powershell
  gh repo create ZayshaaCodes/zPadder --public --source . --push `
    --description "Map game controller input to keyboard, mouse or an Xbox controller. An open-source Xpadder replacement for Windows." `
    --homepage https://github.com/ZayshaaCodes/zPadder
  gh repo edit ZayshaaCodes/zPadder --add-topic xpadder,gamepad,controller,remapping,keyboard-mapping,dualshock4,windows,rust,egui `
    --enable-discussions=false --enable-wiki=false
  gh api -X PUT repos/ZayshaaCodes/zPadder/private-vulnerability-reporting
  ```
- [ ] **Repository settings** on GitHub:
  - Settings > Actions > General: workflow permissions "Read repository contents".
  - Settings > Branches: protect `main` (require the CI checks; no force pushes).
  - Settings > Moderation options > Reported content: accept reports from all users, so
    "Report content" reaches the maintainers (see `CODE_OF_CONDUCT.md`).
- [ ] **First CI run.** Check that every CI job passes on the first push. Nothing has run on
      GitHub's runners yet, in particular building SDL3 on `windows-latest`.
- [ ] **Code signing.** Unsigned input tools get SmartScreen warnings and antivirus false
      positives. Apply to the SignPath Foundation (free for open-source projects). It needs:
  - a public repository;
  - CI-built artifacts;
  - a code-signing policy naming the people who approve releases;
  - the privacy statement in the README.

  Once accepted, add the signing step marked in `.github/workflows/release.yml`.
- [ ] **Clean-machine test.** Test the zip on a clean Windows 10 and 11 install (Windows Sandbox
      is enough). It needs no Visual C++ runtime: check that it starts, that SmartScreen behaves
      as expected, and that a controller maps in Notepad.

### Each release

1. Run the hardware and in-game checks in [docs/roadmap.md](../docs/roadmap.md) that the release touches.
2. Update `CHANGELOG.md`: move "Unreleased" under the new version, with the date.
3. Bump `version` in `[workspace.package]` in `Cargo.toml`, and run `cargo check` so
   `Cargo.lock` follows.
4. Commit, then tag: `git tag v<version>` and `git push origin v<version>`.
5. The **Release** workflow:
   1. checks that the tag matches the version;
   2. runs the tests;
   3. builds with `--locked`;
   4. packages the zip with third-party licences, writing `SHA256SUMS.txt` and a symbols zip
      when there are PDBs;
   5. opens a **draft** GitHub release.
6. Download the draft's zip, check `SHA256SUMS.txt`, run it, then publish the release.

### Building a zip locally

```powershell
cargo install cargo-about --locked --features cli   # once
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
```

The script refuses a dirty working tree (`-AllowDirty` overrides that for test builds). It builds
in `target\package`, so a running `zpadder.exe` doesn't get in the way. A zip built locally is
unsigned.
