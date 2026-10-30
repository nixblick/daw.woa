//! Plugin-Prozess von WOA (Paket 33): lädt ein CLAP-Plugin abseits der Oberfläche. Wird von WOA
//! gestartet, nicht von Hand.

fn main() {
    std::process::exit(woa_plugins::child::main_from_args());
}
