# Project website

The GitHub Pages site is plain HTML, CSS, and JavaScript. Preview from the repository root:

```sh
python3 -m http.server 8000 --directory site
```

Open `http://localhost:8000`. Keep detailed instructions in the repository and link to them from the site.

The page illustrates the offline fixture in `examples/minimal`; it does not execute Rust or call a model. The source, branch choices, and assertions come from `examples/minimal/src/lib.rs` and `scripts/check_minimal.py`. The extracted conditions are `support::positive(value)` being true or false. The context excerpt marks the omitted `threshold` body explicitly; its arithmetic behavior is explained using the original source, not presented as a normalized analysis result.

The optional path walkthrough highlights source, branch, and assertion in sequence. It can be stopped or replayed; changing the input cancels the current sequence. At the single-column breakpoint (760px and below), or with reduced-motion preferences, **Show path summary** displays the complete explanation immediately. Switching into either mode ends an active sequence. Without JavaScript, the fixed example and native context disclosure remain available, while inactive controls stay hidden.

The `nested::double` compilation repair is a separate fixed-response example from that script. Status labels describe the fixture checks, not a live execution or a measurement of model quality. Display snippets omit test wrappers and helper assertions.

The Pages workflow publishes only this directory after relevant changes reach `main`. In repository **Settings → Pages**, select **GitHub Actions** as the source. Project-relative asset URLs work under `/PALM/`.

The self-hosted Sora semibold font is from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/sora), licensed under the [SIL Open Font License](assets/OFL-Sora.md). The geometric PALM mark and interface icons are part of this project's MIT-licensed source.

Sharing metadata uses the canonical URL `https://cppbear.github.io/PALM/` and `assets/social-preview.png`. The 1200 × 630 PNG is a browser export of the editable `assets/social-preview.svg`, which embeds the same licensed Sora font. To refresh it, open the SVG in a browser at 1200 × 630, wait for fonts to load, and capture the viewport at device scale 1. Keep the image dimensions and descriptions in `index.html` in sync. These assets are static; no build step is needed to serve the site. Social-platform previews can be checked after deployment.

The research section links to the maintained paper, poster, presentation, and citation. The poster and presentation describe the paper’s experiments rather than a new evaluation of the maintained implementation.
