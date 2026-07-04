# macOS packaging assets

Source assets for the Greviewer macOS app icon. Consumed by `bin/bundle`.

## Contents

- `AppIcon.iconset/` — the macOS iconset, named for `iconutil`. **Single source of
  truth** for the app icon.
- `icon.svg` — vector master the iconset PNGs were rendered from. Kept for future
  re-exports; not used by the build.

## How the icon is built

`bin/bundle` runs `iconutil -c icns AppIcon.iconset` to generate `AppIcon.icns` and
places it in `Greviewer.app/Contents/Resources/`. The `.icns` is a generated artifact
and is not committed.

## Regenerating the iconset from the master

After editing `icon.svg`, re-render each `AppIcon.iconset` size (16, 32, 64, 128, 256,
512, 1024 px — i.e. every `NxN` and `NxN@2x` file above).
