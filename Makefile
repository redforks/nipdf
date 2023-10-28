dump-page-to-png:
	cargo run -p nipdf-dump -- page -f $(f) --png $(n) > /tmp/foo.png 2>/tmp/log && feh /tmp/foo.png
