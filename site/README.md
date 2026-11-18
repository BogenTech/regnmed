# regnmed.no — marketing site and documentation

Two static Astro sites, built and deployed by `.github/workflows/site.yml`.

| Folder | Site | Stack | Hosted |
| --- | --- | --- | --- |
| `www/` | https://www.regnmed.no | Astro, hand-written CSS, two small scripts (the light/dark toggle, the ledger demo) | this repo's GitHub Pages |
| `docs/` | https://docs.regnmed.no | Astro Starlight (search by Pagefind) | `BogenTech/regnmed-docs` Pages |
| `screens/` | — | real portal screenshots, source of truth | copied into each site at build |

The app itself stays at https://regnmed.no (portal), `api.regnmed.no` and `id.regnmed.no`.

## Develop

```sh
cd site/www  && npm ci && npm run dev    # port 4321
cd site/docs && npm ci && npm run dev
```

`docs/` imports the audit-facing `../docs/*.md` as "Teknisk dokumentasjon" at every build
(`site/docs/scripts/import-technical.mjs`; the copy is gitignored). `brukerveiledning/` and
`developers/` (also translated to English under `en/`) are written by hand. Screenshots: `site/screens/README.md`.

## One-time setup

1. Repo settings → Pages → Source: **GitHub Actions**; custom domain `www.regnmed.no`.
2. Create `BogenTech/regnmed-docs` (public, empty `main`), add a write deploy key, store its
   private half as secret `DOCS_DEPLOY_KEY` here; in that repo set Pages to deploy from `main`
   with custom domain `docs.regnmed.no`.
3. DNS at Domeneshop: `www CNAME bogentech.github.io.` and `docs CNAME bogentech.github.io.`
   (apex `regnmed.no` is unchanged: it serves the portal).

## Why two repos

GitHub Pages serves one custom domain per repository. The source stays here so docs change in
the same PR as the code they describe.
