kernel := "target/riscv64gc-unknown-none-elf/debug/asterinas-osdk-bin"

gdb:
  gdb \
    -ex "source ../legacy/gef/gef.py" \
    -ex "source ../legacy/gdb-pt-dump/pt.py" \
    -ex "file {{kernel}}" \
    -ex "target remote 127.0.0.1:1234" \
    -ex "b __aster_panic_handler" \
    -ex "b riscv/trap.rs:23"
