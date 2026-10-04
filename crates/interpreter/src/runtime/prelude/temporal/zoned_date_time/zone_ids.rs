//! Primary time-zone identifiers for equality (CLDR 48).
//!
//! Generated from common/bcp47/timezone.xml at:
//! https://github.com/unicode-org/cldr/tree/release-48
//! Each alias maps to `iana`, or the first alias when `iana` is absent.
//! ECMAScript also makes Etc/UTC, Etc/GMT and GMT aliases of UTC.
//! Country-specific primary zones remain distinct even with identical rules.
//! Copyright Unicode, Inc. See CLDR-LICENSE in this directory.

pub(super) fn primary(identifier: &str) -> &str {
    let entry = ALIASES.binary_search_by(|(alias, _)| {
        alias
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(identifier.bytes().map(|b| b.to_ascii_lowercase()))
    });
    entry
        .ok()
        .and_then(|index| ALIASES.get(index))
        .map_or(identifier, |(_, primary)| *primary)
}

const ALIASES: &[(&str, &str)] = &[
    ("Africa/Asmera", "Africa/Asmara"),
    ("Africa/Timbuktu", "Africa/Bamako"),
    (
        "America/Argentina/ComodRivadavia",
        "America/Argentina/Catamarca",
    ),
    ("America/Atka", "America/Adak"),
    ("America/Buenos_Aires", "America/Argentina/Buenos_Aires"),
    ("America/Catamarca", "America/Argentina/Catamarca"),
    ("America/Coral_Harbour", "America/Atikokan"),
    ("America/Cordoba", "America/Argentina/Cordoba"),
    ("America/Ensenada", "America/Tijuana"),
    ("America/Fort_Wayne", "America/Indiana/Indianapolis"),
    ("America/Godthab", "America/Nuuk"),
    ("America/Indianapolis", "America/Indiana/Indianapolis"),
    ("America/Jujuy", "America/Argentina/Jujuy"),
    ("America/Knox_IN", "America/Indiana/Knox"),
    ("America/Louisville", "America/Kentucky/Louisville"),
    ("America/Mendoza", "America/Argentina/Mendoza"),
    ("America/Montreal", "America/Toronto"),
    ("America/Nipigon", "America/Toronto"),
    ("America/Pangnirtung", "America/Iqaluit"),
    ("America/Porto_Acre", "America/Rio_Branco"),
    ("America/Rainy_River", "America/Winnipeg"),
    ("America/Rosario", "America/Argentina/Cordoba"),
    ("America/Santa_Isabel", "America/Tijuana"),
    ("America/Shiprock", "America/Denver"),
    ("America/Thunder_Bay", "America/Toronto"),
    ("America/Virgin", "America/St_Thomas"),
    ("America/Yellowknife", "America/Edmonton"),
    ("Antarctica/South_Pole", "Antarctica/McMurdo"),
    ("Asia/Ashkhabad", "Asia/Ashgabat"),
    ("Asia/Calcutta", "Asia/Kolkata"),
    ("Asia/Choibalsan", "Asia/Ulaanbaatar"),
    ("Asia/Chongqing", "Asia/Shanghai"),
    ("Asia/Chungking", "Asia/Shanghai"),
    ("Asia/Dacca", "Asia/Dhaka"),
    ("Asia/Harbin", "Asia/Shanghai"),
    ("Asia/Istanbul", "Europe/Istanbul"),
    ("Asia/Kashgar", "Asia/Urumqi"),
    ("Asia/Katmandu", "Asia/Kathmandu"),
    ("Asia/Macao", "Asia/Macau"),
    ("Asia/Rangoon", "Asia/Yangon"),
    ("Asia/Saigon", "Asia/Ho_Chi_Minh"),
    ("Asia/Tel_Aviv", "Asia/Jerusalem"),
    ("Asia/Thimbu", "Asia/Thimphu"),
    ("Asia/Ujung_Pandang", "Asia/Makassar"),
    ("Asia/Ulan_Bator", "Asia/Ulaanbaatar"),
    ("Atlantic/Faeroe", "Atlantic/Faroe"),
    ("Atlantic/Jan_Mayen", "Arctic/Longyearbyen"),
    ("Australia/ACT", "Australia/Sydney"),
    ("Australia/Canberra", "Australia/Sydney"),
    ("Australia/Currie", "Australia/Hobart"),
    ("Australia/LHI", "Australia/Lord_Howe"),
    ("Australia/North", "Australia/Darwin"),
    ("Australia/NSW", "Australia/Sydney"),
    ("Australia/Queensland", "Australia/Brisbane"),
    ("Australia/South", "Australia/Adelaide"),
    ("Australia/Tasmania", "Australia/Hobart"),
    ("Australia/Victoria", "Australia/Melbourne"),
    ("Australia/West", "Australia/Perth"),
    ("Australia/Yancowinna", "Australia/Broken_Hill"),
    ("Brazil/Acre", "America/Rio_Branco"),
    ("Brazil/DeNoronha", "America/Noronha"),
    ("Brazil/East", "America/Sao_Paulo"),
    ("Brazil/West", "America/Manaus"),
    ("Canada/Atlantic", "America/Halifax"),
    ("Canada/Central", "America/Winnipeg"),
    ("Canada/East-Saskatchewan", "America/Regina"),
    ("Canada/Eastern", "America/Toronto"),
    ("Canada/Mountain", "America/Edmonton"),
    ("Canada/Newfoundland", "America/St_Johns"),
    ("Canada/Pacific", "America/Vancouver"),
    ("Canada/Saskatchewan", "America/Regina"),
    ("Canada/Yukon", "America/Whitehorse"),
    ("CET", "Europe/Brussels"),
    ("Chile/Continental", "America/Santiago"),
    ("Chile/EasterIsland", "Pacific/Easter"),
    ("CST6CDT", "America/Chicago"),
    ("Cuba", "America/Havana"),
    ("EET", "Europe/Athens"),
    ("Egypt", "Africa/Cairo"),
    ("Eire", "Europe/Dublin"),
    ("EST", "America/Panama"),
    ("EST5EDT", "America/New_York"),
    ("Etc/GMT", "UTC"),
    ("Etc/GMT+0", "UTC"),
    ("Etc/GMT-0", "UTC"),
    ("Etc/GMT0", "UTC"),
    ("Etc/Greenwich", "UTC"),
    ("Etc/UCT", "UTC"),
    ("Etc/Universal", "UTC"),
    ("Etc/UTC", "UTC"),
    ("Etc/Zulu", "UTC"),
    ("Europe/Belfast", "Europe/London"),
    ("Europe/Kiev", "Europe/Kyiv"),
    ("Europe/Nicosia", "Asia/Nicosia"),
    ("Europe/Tiraspol", "Europe/Chisinau"),
    ("Europe/Uzhgorod", "Europe/Kyiv"),
    ("Europe/Zaporozhye", "Europe/Kyiv"),
    ("Factory", "Etc/Unknown"),
    ("GB", "Europe/London"),
    ("GB-Eire", "Europe/London"),
    ("GMT", "UTC"),
    ("GMT+0", "UTC"),
    ("GMT-0", "UTC"),
    ("GMT0", "UTC"),
    ("Greenwich", "UTC"),
    ("Hongkong", "Asia/Hong_Kong"),
    ("HST", "Pacific/Honolulu"),
    ("Iceland", "Atlantic/Reykjavik"),
    ("Iran", "Asia/Tehran"),
    ("Israel", "Asia/Jerusalem"),
    ("Jamaica", "America/Jamaica"),
    ("Japan", "Asia/Tokyo"),
    ("Kwajalein", "Pacific/Kwajalein"),
    ("Libya", "Africa/Tripoli"),
    ("MET", "Europe/Brussels"),
    ("Mexico/BajaNorte", "America/Tijuana"),
    ("Mexico/BajaSur", "America/Mazatlan"),
    ("Mexico/General", "America/Mexico_City"),
    ("MST", "America/Phoenix"),
    ("MST7MDT", "America/Denver"),
    ("Navajo", "America/Denver"),
    ("NZ", "Pacific/Auckland"),
    ("NZ-CHAT", "Pacific/Chatham"),
    ("Pacific/Enderbury", "Pacific/Kanton"),
    ("Pacific/Johnston", "Pacific/Honolulu"),
    ("Pacific/Ponape", "Pacific/Pohnpei"),
    ("Pacific/Samoa", "Pacific/Pago_Pago"),
    ("Pacific/Truk", "Pacific/Chuuk"),
    ("Pacific/Yap", "Pacific/Chuuk"),
    ("Poland", "Europe/Warsaw"),
    ("Portugal", "Europe/Lisbon"),
    ("PRC", "Asia/Shanghai"),
    ("PST8PDT", "America/Los_Angeles"),
    ("ROC", "Asia/Taipei"),
    ("ROK", "Asia/Seoul"),
    ("Singapore", "Asia/Singapore"),
    ("Turkey", "Europe/Istanbul"),
    ("UCT", "UTC"),
    ("Universal", "UTC"),
    ("US/Alaska", "America/Anchorage"),
    ("US/Aleutian", "America/Adak"),
    ("US/Arizona", "America/Phoenix"),
    ("US/Central", "America/Chicago"),
    ("US/East-Indiana", "America/Indiana/Indianapolis"),
    ("US/Eastern", "America/New_York"),
    ("US/Hawaii", "Pacific/Honolulu"),
    ("US/Indiana-Starke", "America/Indiana/Knox"),
    ("US/Michigan", "America/Detroit"),
    ("US/Mountain", "America/Denver"),
    ("US/Pacific", "America/Los_Angeles"),
    ("US/Pacific-New", "America/Los_Angeles"),
    ("US/Samoa", "Pacific/Pago_Pago"),
    ("W-SU", "Europe/Moscow"),
    ("WET", "Europe/Lisbon"),
    ("Zulu", "UTC"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_sorted_unique_and_resolve_in_one_lookup() {
        for pair in ALIASES.windows(2) {
            assert!(pair[0].0.to_ascii_lowercase() < pair[1].0.to_ascii_lowercase());
        }
        for (alias, expected) in ALIASES {
            assert_eq!(primary(alias), *expected);
            assert_eq!(primary(&alias.to_ascii_uppercase()), *expected);
            assert_eq!(primary(expected), *expected);
        }
        assert_eq!(primary("US/Eastern"), "America/New_York");
        assert_eq!(primary("Iceland"), "Atlantic/Reykjavik");
        assert_eq!(primary("Atlantic/Reykjavik"), "Atlantic/Reykjavik");
        assert_eq!(primary("Africa/Abidjan"), "Africa/Abidjan");
        assert_eq!(primary("+00:00"), "+00:00");
    }
}
