# PLUME showcase site

Static product site for PLUME, served from `site/` by GitHub Pages.

## Structure

```
site/
  index.html            Product page, gallery, downloads
  404.html              Pages not-found page
  assets/css/site.css   Design system and responsive layout
  assets/js/site.js     Navigation, reveals, ribbon demo, lightbox, copy
  assets/img/           PLUME renders, UI screenshots, Open Graph image
  assets/favicon.svg    PLUME mark
```

## Local preview

There is no build step. Serve the folder over HTTP so the relative asset paths and
the lightbox behave like production:

```sh
python -m http.server 8000 --directory site
```

Then open <http://localhost:8000/>.

## Deployment

`.github/workflows/pages.yml` uploads `site/` as a Pages artifact and deploys it on
every push to `main` that touches `site/`. The repository setting must use
**GitHub Actions** as the Pages source (`build_type: workflow`).

Production URL: <https://aamirenic.github.io/Plume/>

## Editing notes

- Keep all asset paths relative (`assets/...`) so the site works under the
  `/Plume/` project path.
- The 404 page uses absolute `/Plume/` paths because GitHub Pages serves it from
  the domain root for unknown URLs.
- Screenshots in `assets/img/` are PLUME output, not stock art. Re-export them
  from the app when the interface changes.
- The Windows checksum in `index.html` must match the asset on release
  `v0.1.0`.
