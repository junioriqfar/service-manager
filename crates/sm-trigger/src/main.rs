use std::env;

fn main() {
    let values: Vec<String> = env::args().skip(1).collect();
    let args = match sm_trigger::parse_args(&values) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            sm_trigger::print_usage();
            std::process::exit(2);
        }
    };
    if args.help {
        sm_trigger::print_usage();
        return;
    }
    std::process::exit(sm_trigger::run(&args));
}
