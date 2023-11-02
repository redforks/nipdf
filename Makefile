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

CLIPPY_RULES = -W clippy::clone_on_ref_ptr -W clippy::empty_structs_with_brackets -D clippy::dbg_macro -W clippy::get_unwrap -W clippy::if_then_some_else_none -W clippy::large_include_file -W clippy::rc_buffer -W clippy::redundant_type_annotations -W clippy::verbose_file_reads -W clippy::collection_is_never_read -W clippy::debug_assert_with_mut_call -W clippy::imprecise_flops -W clippy::manual_clamp -W clippy::needless_collect -W clippy::or_fun_call -W clippy::redundant_clone

clippy:
	cargo clippy --all-targets -- $(CLIPPY_RULES)

clippy-fix:
	cargo clippy --fix -- $(CLIPPY_RULES)