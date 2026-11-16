use std::env;

fn main() {
    // Dipakai sebagai SSH_ASKPASS helper oleh GUI Remote (password mode).
    if env::var_os("SM_ASKPASS").is_some() {
        let password = env::var("SM_ASKPASS_PASSWORD").unwrap_or_default();
        println!("{password}");
        return;
    }

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
