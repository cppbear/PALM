# Project website

The GitHub Pages site is plain HTML, CSS, and JavaScript. Preview from the repository root:

```sh
python3 -m http.server 8000 --directory site
```

Open `http://localhost:8000`. The branch selector is an illustration of `examples/minimal`, not a live model or Rust execution. Keep detailed instructions in the repository and link to them from the site.

The Pages workflow publishes only this directory after relevant changes reach `main`. In repository **Settings → Pages**, select **GitHub Actions** as the source. Project-relative asset URLs work under `/PALM/`.

The self-hosted Sora semibold font is from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/sora), licensed under the [SIL Open Font License](assets/OFL-Sora.md). The geometric PALM mark is part of this project's MIT-licensed source.
