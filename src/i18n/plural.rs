//! CLDR cardinal plural rules (unicode.org/cldr, "Language Plural Rules") for whole numbers: which form
//! of a counted word a language uses for `n`. Counts in the interface are never fractions, so the rules
//! that only look at the fraction digits are left out. Unknown languages get the English rule.

/// A CLDR plural category; tdesktop keys carry it as a suffix (`lng_xxx#few`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Zero = 0,
    One = 1,
    Two = 2,
    Few = 3,
    Many = 4,
    Other = 5,
}

/// The category of `n` in the language `lang` (a Telegram or system code: `uk`, `pt-br`, `zh-hans`…).
pub fn plural_category(lang: &str, n: u64) -> Category {
    use Category::*;
    let lang = lang.to_ascii_lowercase();
    let base = lang.split(['-', '_']).next().unwrap_or_default();
    let (n10, n100) = (n % 10, n % 100);
    let teen = (11..=19).contains(&n100);
    // The East Slavic and Serbo-Croatian pattern: 1, 21, 31… / 2–4, 22–24… / the rest.
    let slavic_one = n10 == 1 && n100 != 11;
    let slavic_few = (2..=4).contains(&n10) && !(12..=14).contains(&n100);
    // Spanish, French, Italian, Portuguese, Catalan: "de" forms for exact millions ("1 million de…").
    let million = n != 0 && n % 1_000_000 == 0;
    match base {
        "ja" | "ko" | "zh" | "id" | "ms" | "th" | "vi" | "my" | "lo" | "km" | "jv" | "yue" => Other,
        "uk" | "ru" | "be" => match () {
            _ if slavic_one => One,
            _ if slavic_few => Few,
            _ => Many,
        },
        "pl" => match () {
            _ if n == 1 => One,
            _ if slavic_few => Few,
            _ => Many,
        },
        "hr" | "sr" | "bs" => match () {
            _ if slavic_one => One,
            _ if slavic_few => Few,
            _ => Other,
        },
        "cs" | "sk" => match n {
            1 => One,
            2..=4 => Few,
            _ => Other,
        },
        "sl" => match n100 {
            1 => One,
            2 => Two,
            3 | 4 => Few,
            _ => Other,
        },
        "lt" => match () {
            _ if n10 == 1 && !teen => One,
            _ if n10 >= 2 && !teen => Few,
            _ => Other,
        },
        "lv" => match () {
            _ if n10 == 0 || teen => Zero,
            _ if slavic_one => One,
            _ => Other,
        },
        "ro" | "mo" => match () {
            _ if n == 1 => One,
            _ if n == 0 || (1..=19).contains(&n100) => Few,
            _ => Other,
        },
        "ar" => match () {
            _ if n == 0 => Zero,
            _ if n == 1 => One,
            _ if n == 2 => Two,
            _ if (3..=10).contains(&n100) => Few,
            _ if (11..=99).contains(&n100) => Many,
            _ => Other,
        },
        "he" | "iw" => match n {
            1 => One,
            2 => Two,
            _ => Other,
        },
        "fr" => match () {
            _ if n <= 1 => One,
            _ if million => Many,
            _ => Other,
        },
        // Brazilian: 0 and 1 are singular; European Portuguese only 1.
        "pt" if lang != "pt-pt" && lang != "pt_pt" => match () {
            _ if n <= 1 => One,
            _ if million => Many,
            _ => Other,
        },
        "es" | "it" | "ca" | "pt" => match () {
            _ if n == 1 => One,
            _ if million => Many,
            _ => Other,
        },
        "fa" | "hi" | "bn" | "gu" | "kn" | "mr" | "zu" | "am" => match n {
            0 | 1 => One,
            _ => Other,
        },
        "mk" => match () {
            _ if slavic_one => One,
            _ => Other,
        },
        // en, de, nl, sv, nb, da, fi, et, tr, hu, uz, kk, el, bg, az, ka, ky… and anything unknown.
        _ => match n {
            1 => One,
            _ => Other,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::Category::*;
    use super::*;

    fn cats(lang: &str, ns: &[u64]) -> Vec<Category> {
        ns.iter().map(|n| plural_category(lang, *n)).collect()
    }

    #[test]
    fn east_slavic() {
        for lang in ["uk", "ru", "be"] {
            assert_eq!(cats(lang, &[0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 1000]), [Many, One, Few, Few, Many, Many, Many, Many, One, Few, Many, One, Many, Many], "{lang}");
        }
    }

    #[test]
    fn west_and_south_slavic() {
        assert_eq!(cats("pl", &[0, 1, 2, 5, 12, 21, 22, 25, 102]), [Many, One, Few, Many, Many, Many, Few, Many, Few]);
        assert_eq!(cats("cs", &[0, 1, 2, 4, 5, 22]), [Other, One, Few, Few, Other, Other]);
        assert_eq!(cats("sk", &[1, 3, 10]), [One, Few, Other]);
        assert_eq!(cats("hr", &[1, 2, 5, 11, 21, 22]), [One, Few, Other, Other, One, Few]);
        assert_eq!(cats("sr-latn", &[1, 3, 13]), [One, Few, Other]);
        assert_eq!(cats("sl", &[1, 2, 3, 4, 5, 101, 102, 103]), [One, Two, Few, Few, Other, One, Two, Few]);
    }

    #[test]
    fn baltic_and_romanian() {
        assert_eq!(cats("lt", &[1, 2, 9, 10, 11, 19, 21, 22]), [One, Few, Few, Other, Other, Other, One, Few]);
        assert_eq!(cats("lv", &[0, 1, 2, 10, 11, 21, 111]), [Zero, One, Other, Zero, Zero, One, Zero]);
        assert_eq!(cats("ro", &[0, 1, 2, 19, 20, 101, 120]), [Few, One, Few, Few, Other, Few, Other]);
    }

    #[test]
    fn semitic() {
        assert_eq!(cats("ar", &[0, 1, 2, 3, 10, 11, 99, 100, 102, 103, 111]), [Zero, One, Two, Few, Few, Many, Many, Other, Other, Few, Many]);
        assert_eq!(cats("he", &[1, 2, 3, 10, 20]), [One, Two, Other, Other, Other]);
    }

    #[test]
    fn romance_and_germanic() {
        for lang in ["en", "de", "nl", "sv", "nb", "da", "fi", "tr", "hu", "uz", "kk"] {
            assert_eq!(cats(lang, &[0, 1, 2, 21]), [Other, One, Other, Other], "{lang}");
        }
        assert_eq!(cats("fr", &[0, 1, 2, 1_000_000]), [One, One, Other, Many]);
        assert_eq!(cats("pt-br", &[0, 1, 2]), [One, One, Other]);
        assert_eq!(cats("pt-pt", &[0, 1, 2]), [Other, One, Other]);
        assert_eq!(cats("es", &[0, 1, 2, 2_000_000]), [Other, One, Other, Many]);
        assert_eq!(cats("it", &[1, 5]), [One, Other]);
        assert_eq!(cats("fa", &[0, 1, 2]), [One, One, Other]);
    }

    #[test]
    fn no_plurals() {
        for lang in ["ja", "ko", "zh-hans", "id", "ms"] {
            assert_eq!(cats(lang, &[0, 1, 2, 5, 21]), [Other; 5], "{lang}");
        }
    }
}
