use super::Expression;

#[test]
fn select_inputs_and_mapping_follow_wasm_stack_order() {
    let expression = Expression::Select {
        condition: String::from("condition"),
        when_true: String::from("true"),
        when_false: String::from("false"),
    };
    let forward: Vec<_> = expression.inputs().map(String::as_str).collect();
    let backward: Vec<_> = expression.inputs().rev().map(String::as_str).collect();
    assert_eq!(forward, ["true", "false", "condition"]);
    assert_eq!(backward, ["condition", "false", "true"]);

    let mut visited = Vec::new();
    let borrowed = expression.map(|input| {
        visited.push(input.as_str());
        input.as_str()
    });
    assert_eq!(visited, ["true", "false", "condition"]);
    assert!(matches!(
        borrowed,
        Expression::Select {
            condition: "condition",
            when_true: "true",
            when_false: "false",
        }
    ));
}

#[test]
fn input_mapping_stops_at_the_first_error() {
    let expression = Expression::Select {
        condition: 3,
        when_true: 1,
        when_false: 2,
    };
    let mut visited = Vec::new();
    let result = expression.try_map(|&input| {
        visited.push(input);
        if input == 2 {
            Err("unavailable input")
        } else {
            Ok(input)
        }
    });
    assert_eq!(visited, [1, 2]);
    assert!(matches!(result, Err("unavailable input")));
}
