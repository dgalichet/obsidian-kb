# site/

Static documentation mini-site for **obsidian-kb**, implemented from the Claude
Design project *"Mini site de documentation plugin"*.

Single self-contained `index.html` — no build step, no runtime dependencies. It
reproduces the design 1:1:

- **Hash routing** (`#/`, `#/docs`) — a small vanilla-JS SPA, no framework.
- **FR / EN i18n** with a header toggle, persisted in `localStorage`
  (`okb-lang`).
- **Dark / light theme** with a header toggle (moon/sun), persisted in
  `localStorage` (`okb-theme`) — CSS variables, pre-paint script to avoid a
  flash of the wrong theme.
- Space Grotesk / IBM Plex Sans / IBM Plex Mono (Google Fonts), animated
  retrieval pipeline, feature grid, search-mode cards, docs sidebar (incl. the
  Obsidian plugin section).

> The blog / article section from the design is intentionally left out for now
> (drafts to be reworked before publishing). To restore it, re-sync the blog
> route, views and `blog`/`articles`/`article` dictionary entries from the
> design project.

The brand icons (`assets/okb-icon.png`, `assets/okb-icon-light.png`) are square
crops of the repo banner artwork (`../assets/obsidian-kb-logo*.png`), resized to
128 px. The article hero and author avatar use CSS gradient placeholders — swap
in a real screenshot / photo when available.

## Preview

Open `index.html` directly, or serve it:

```
python3 -m http.server -d site 8080
# → http://localhost:8080/
```

## Content

All copy lives in the `DICTS` object at the bottom of `index.html` (French and
English). Commands, install steps and search modes are kept in sync with the
project `README.md` and `docs/`.
