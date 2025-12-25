//! Developer utility to benchmark analysis throughput and library DB queries.

mod bench;

fn main() {
    if let Err(err) = bench::run(std::env::args().skip(1).collect()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
