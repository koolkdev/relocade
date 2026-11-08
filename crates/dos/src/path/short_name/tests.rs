use super::{search_pattern, ShortName};

#[test]
fn pathname_fields_pad_fold_case_and_truncate_independently() {
    for (input, key, dotted) in [
        (b"ddfnt2.car".as_slice(), *b"DDFNT2  CAR", "DDFNT2.CAR"),
        (b"DDFNT2  .CAR", *b"DDFNT2  CAR", "DDFNT2.CAR"),
        (b"DDFNT2   .CAR ", *b"DDFNT2  CAR", "DDFNT2.CAR"),
        (b"abcdefghi.jklm", *b"ABCDEFGHJKL", "ABCDEFGH.JKL"),
        (b"ABCDEFGHIJK", *b"ABCDEFGH   ", "ABCDEFGH"),
        (b"AB CD   .X  ", *b"AB CD   X  ", "AB CD.X"),
        (b"A.B C", *b"A       B C", "A.B C"),
        (b"A. X ", *b"A        X ", "A. X"),
        (b"FILE.   ", *b"FILE       ", "FILE"),
        (b".", *b".          ", "."),
        (b"..", *b"..         ", ".."),
    ] {
        let name = ShortName::parse(input).unwrap();
        assert_eq!(name.0, key, "{input:?}");
        assert_eq!(name.to_dotted(), dotted, "{input:?}");
        assert_eq!(search_pattern(input), Some(key), "{input:?}");
    }
}

#[test]
fn invalid_characters_and_empty_bases_do_not_become_filename_aliases() {
    for input in [
        b"".as_slice(),
        b" ",
        b" NAME.TXT",
        b".TXT",
        b"...",
        b"A.B.C",
        b"A\t.TXT",
        b"A\0.TXT",
        b"A\x80.TXT",
        b"A/B",
        b"A\\B",
        b"A:B",
        b"A\"B",
        b"A;B",
        b"A,B",
        b"A+B",
        b"A=B",
        b"A[B",
        b"A]B",
        b"A<B",
        b"A>B",
        b"A|B",
        b"ABCDEFGH?.X+",
        b"A*.TXT/",
    ] {
        assert_eq!(ShortName::parse(input), None, "{input:?}");
        assert_eq!(search_pattern(input), None, "{input:?}");
    }
}

#[test]
fn search_stars_fill_their_field_and_question_marks_match_padding() {
    for (input, expected) in [
        (b"*".as_slice(), *b"????????   "),
        (b"*.*", *b"???????????"),
        (b"A*B.C*D", *b"A???????C??"),
        (b"A?      .X  ", *b"A?      X  "),
        (b"ABCDEFGH*.XYZ*", *b"ABCDEFGHXYZ"),
    ] {
        assert_eq!(search_pattern(input), Some(expected), "{input:?}");
    }
    let pattern = search_pattern(b"A?      .X").unwrap();
    assert!(ShortName::parse(b"A.X").unwrap().matches(&pattern));
    assert!(ShortName::parse(b"AB.X").unwrap().matches(&pattern));
    assert!(!ShortName::parse(b"ABC.X").unwrap().matches(&pattern));
    assert_eq!(ShortName::parse(b"A*.X"), None);
    assert_eq!(ShortName::parse(b"A?.X"), None);
    assert_eq!(
        ShortName::parse(b"ABCDEFGH*.XYZ*").unwrap().to_dotted(),
        "ABCDEFGH.XYZ"
    );
}

#[test]
fn directory_entries_require_canonical_names_without_inventing_long_name_aliases() {
    for name in ["DDFNT2.CAR", "AB CD.X", "PLAIN", ".", ".."] {
        assert_eq!(ShortName::from_entry(name).unwrap().to_dotted(), name);
    }
    for name in [
        "TOO-LONG-NAME.DAT",
        "FILE.LONG",
        "DDFNT2  .CAR",
        "file.txt",
        "FILE.",
    ] {
        assert_eq!(ShortName::from_entry(name), None, "{name}");
    }
}
