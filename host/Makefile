# PC-side tests: the CPU test suites and the disassembler test.
#
#   make test         run the CPU test suites against src/cpu.rs
#   make test-disasm  check the disassembler against 64tass (needs 64tass)
#
# The simulator is built with cargo: cargo run --release --bin sim6502

CARGO ?= cargo

# The test programs come from the upstream Fake6502 repository.
fake6502-upstream:
	git clone --depth 1 https://github.com/ivop/fake6502.git $@

test: fake6502-upstream
	$(CARGO) run --quiet --release --example cputest -- fake6502-upstream | tee cputest.log
	@! grep -q FAIL cputest.log

test-disasm:
	python3 test_disasm.py

clean:
	rm -f cputest.log

distclean: clean
	rm -rf fake6502-upstream

.PHONY: test test-disasm clean distclean
