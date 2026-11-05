# Contributing to Bramble Office

Thanks for wanting to help. This repository is where Bramble Office is developed: Keydeck, Letters,
Values, Bramble Notes and Bramble Calculator, for the desktop and for Android.

## Ways to help

- **Report a problem or suggest a feature** in [Issues](https://github.com/bramble-apps/bramble-office/issues).
  Say which app and system you're on, what you did and what happened, and attach a file or screenshot if you can.
- **Fix something or add something** with a pull request (below).
- **Android ports** are especially welcome: `letters_mobile/`, `keydeck_mobile/`, `values_mobile/` and
  `notes_mobile/` are Flutter apps on top of the Rust engines.

## Sending a change

1. Fork this repository and clone your fork.
2. Make a branch for your change: `git switch -c fix-table-borders`.
3. Build and test the app you changed (see [Building from source](README.md#building-from-source)):
   `cargo build --release -p <app>` and `cargo test --workspace` in its folder; for an Android app,
   `flutter analyze` and `flutter test` in its folder.
4. Keep the change focused: one fix or feature per pull request, in the style of the code around it.
5. Open a pull request against `main` and describe what it changes and how you checked it.

Small pull requests get reviewed fastest. For something big, open an issue first so we can agree on the
approach before you put the work in.

## How the code fits together

- `keynote-linux/` holds Keydeck and the crates every app shares (`kn-format`, `kn-model`, `kn-render`,
  `kn-ui`). Values (`numbers-linux/`), Letters (`pages-linux/`) and Bramble Notes (`notes-linux/`) build
  against them by relative path, so keep the folders side by side.
- Fix shared look and behaviour once, in `kn-ui`; each app keeps its own theme and look settings.
- Tests that need Apple-made sample files look for a `testdata/` folder that isn't in this repository;
  without it they're reported as ignored, and everything else runs.

## About this project

Much of Bramble Office's code was written with the help of AI coding tools (Anthropic's Claude), with the
maintainer directing, reviewing and testing it. Commits made with that help say so in a
`Co-Authored-By` line. Contributions written by hand or with your own tools are equally welcome.

## Licence

Bramble Office is GPL-3.0-only. By sending a pull request you agree that your contribution is released
under the same licence.
