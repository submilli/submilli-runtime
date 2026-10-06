use interpreter::Type;
use interpreter::type_size::{TypeLimits, TypeTooLarge};
use interpreter::typechecker::type_param_substitution::TypeParamSubstitution;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn binding_chains_on_small_stack() {
    if std::env::var_os("SUBMILLI_BINDING_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_binding_chains)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    // Stack overflows abort; isolate the regression and bound its running time.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "binding_chains_on_small_stack", "--nocapture"])
        .env("SUBMILLI_BINDING_LIMIT_CHILD", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "binding child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("binding child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check_binding_chains() {
    for count in [2_000, 10_000, 1] {
        let mut substitution = TypeParamSubstitution::new();
        for index in 0..count {
            let target = if index + 1 == count {
                Type::Number
            } else {
                Type::TypeVar(format!("T{}", index + 1))
            };
            substitution.insert(format!("T{index}"), target);
        }
        let result = substitution.apply(&Type::TypeVar("T0".into()), &TypeLimits::default());
        if count == 10_000 {
            assert_eq!(result, Err(TypeTooLarge::Work));
        } else {
            assert_eq!(result, Ok(Type::Number));
            substitution.insert(format!("T{}", count - 1), Type::TypeVar("T0".into()));
            assert_eq!(
                substitution.apply(&Type::TypeVar("T0".into()), &TypeLimits::default()),
                Ok(Type::TypeVar("T0".into()))
            );
        }
    }
}
