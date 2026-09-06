use ry_checker::Checker;
use ry_core::{Mode, RParser};

#[test]
fn assignment_capture_is_optional_and_preserves_diagnostics() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse("test.R", "x <- 1L\nx <- \"later\"\ny <- x + 1L\n")
        .unwrap();
    let mut plain = Checker::new("test.R");
    let expected = plain.check(&file).to_vec();
    assert!(!expected.is_empty());
    assert!(plain.take_assignment_types().is_empty());

    let mut captured = Checker::new("test.R");
    captured.enable_assignment_capture();
    assert_eq!(captured.check(&file), expected);
    let types = captured.take_assignment_types();
    assert_eq!(types.len(), 3);
    assert_eq!(types[0].1.mode, Mode::Integer);
    assert_eq!(types[1].1.mode, Mode::Character);
    assert!(captured.take_assignment_types().is_empty());

    // Starting another check also drops records the caller did not take.
    captured.check(&file);
    let second = parser.parse("next.R", "z <- FALSE\n").unwrap();
    captured.check(&second);
    let types = captured.take_assignment_types();
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].1.mode, Mode::Logical);
    assert_eq!(&second.source[types[0].0.start..types[0].0.end], "z");
}
