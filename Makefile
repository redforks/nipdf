SHELL := /usr/bin/fish

dump-page-to-png:
	RUST_LOG=debug cargo run -p nipdf-dump -- page -f $(f) --png $(n) > /tmp/foo.png 2>/tmp/log && feh /tmp/foo.png

release-build-dump:
	cargo build -p nipdf-dump --release

render-test-1: release-build-dump
	for i in $$(seq 0 1310); echo $$i; if not target/release/nipdf-dump page -f nipdf/sample_files/bizarre/pdfReferenceUpdated.pdf $$i --png > /dev/null 2>/tmp/log; break; end; end

render-test-2: release-build-dump
	for i in $$(seq 0 951); echo $$i; if not target/release/nipdf-dump page -f ~/code.pdf $$i --png > /dev/null 2>/tmp/log; break; end; end

render-test: render-test-1 render-test-2
	# ensure that page rendering not panic
