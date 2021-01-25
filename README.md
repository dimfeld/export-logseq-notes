# Roam Note Exporter

This is a program to take a Roam Research EDN export and convert it into web pages.

## Features

- Selective export: Include and exclude pages based on presence of tags in each page
- Optionally omit lines that only contain links/tags to pages that aren't included.
  - So tag-only lines like `#Articles #Done #Thinking` don't clutter up the page.
- Support block references, block embeds, and page embeds.
- Supports output templates: complete HTML page, text with front matter, or anything else!
- Gathers hashtags in a page for use in the output template 
  - This is configurable to use either a specific "Tags" attribute, or hashtags anywhere in a page.
  - Tags can be excluded

## Configuration

Primary configuration is through a TOML file. You can find an [annotated config file here](https://github.com/dimfeld/export-roam-notes/blob/master/export-roam-notes.toml) and
modify it for your needs.

The program also supports command line arguments to override settings in the config file. `export-roam-notes --help` will show
the arguments available.

## Notable features planned

- [X] Option to only export pages with a certain tag
- [X] When a page links to another exported page, the output contains a link.
- [X] Expands block embeds
- [X] Link block references to original block
- [ ] Translate namespaces into nested directories
- [ ] Option to show backlinks at bottom
- [ ] Autodownload a Roam EDN export file. I might just make it work with roam-to-git for this.


## Acknowledgements

- [edn-rs](https://github.com/naomijub/edn-rs) for the core EDN parsing
- [nom](https://github.com/Geal/nom) for making it easy to write custom parsers
- The Svelte syntax file is imported from `https://github.com/corneliusio/svelte-sublime`.
