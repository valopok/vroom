# vroom

This is a rewrite of the `vroom` userspace NVMe driver with the following changes:

- compilable with `no-std`
- usable with a custom allocator

The goal of the rewrite is to allow the driver to be used within the
[Hermit Unikernel](https://github.com/hermit-os/kernel).

[The thesis of the original `vroom` project by Tuomas Pirhonen](https://db.in.tum.de/people/sites/ellmann/theses/finished/24/pirhonen_writing_an_nvme_driver_in_rust.pdf)
contains some details about the original implementation.


## Disclaimer

This is by no means production-ready.  
Do not use it in critical environments.  
DMA may corrupt memory.


## Build instructions for Linux systems

You will need Rust, as well as its package manager `cargo`.  
The installation instructions can be found at [rustup.rs](https://rustup.rs/).

Huge pages need to be enabled:
```sh
cd vroom
sudo ./setup-hugetlbfs.sh
```

Build the driver, as well as any examples:
```sh
cargo build --release --all-targets
```

Get the PCI address of an NVMe drive with `lspci`.  
The address should be formatted as `0000:00:xx.x` for the program.

Run the `std_pci_huge` example (root privileges are needed for DMA):
```sh
sudo ./target/release/examples/std_pci_huge 0000:00:08.0
```


## Related projects

- [Redox's NVMe driver](https://gitlab.redox-os.org/redox-os/drivers/-/tree/master/storage/nvmed)
