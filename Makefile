dump-page-to-png:
	RUST_LOG=debug cargo run -p nipdf-dump -- page -f $(f) --png $(n) > /tmp/foo.png 2>/tmp/log && feh /tmp/foo.png
