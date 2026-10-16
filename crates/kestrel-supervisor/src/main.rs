use std::collections::BTreeMap;

use kestrel_supervisor::{Stderr, broker, run};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let variables: BTreeMap<String, String> = std::env::vars().collect();
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    std::process::exit(match arguments.split_first() {
        Some((helped, rest)) if helped == "credential" => broker::credential(
            rest.first().map(String::as_str),
            std::io::stdin(),
            std::io::stdout(),
            std::io::stderr(),
            &variables,
        ),
        Some((helped, [real, rest @ ..])) if helped == "gh" => {
            broker::gh(real, rest, std::io::stderr(), &variables)
        }
        _ => run(&Stderr, &variables).await,
    });
}
