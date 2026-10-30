# Packaging Keydeck

Both scripts build the release binary first and write their output into `dist/`.

## Debian / Ubuntu package (.deb)

```bash
packaging/build-deb.sh               # app, icons, app-grid entry
packaging/build-deb.sh --with-themes # also ships Keydeck's themes in /usr/share/keydeck/themes
sudo apt install ./dist/keydeck_0.1.0_amd64.deb
```

Recommends `ffmpeg` (movies) and `curl`. Suggests `yt-dlp` (web videos). Help > Update Video
Support fetches a current yt-dlp and Deno into `~/.local/share/keydeck/bin` without root.

## Portable tarball (any Linux, no root)

```bash
packaging/build-tarball.sh
tar xzf dist/keydeck-0.1.0-linux-x86_64.tar.gz
keydeck-0.1.0-linux-x86_64/install.sh   # installs into ~/.local
```

## What is not packaged

- Keynote's theme artwork library (`~/.local/share/keydeck/resources`): it is Apple's. Keydeck
  finds it there or in `/usr/share/keydeck/resources` if the user provides it.
- Keydeck's own themes contain Unsplash photos (free licence; credits in
  `PHOTO-CREDITS.tsv`). Their layouts derive from Keynote's themes, so check that before
  shipping them publicly with `--with-themes`.
