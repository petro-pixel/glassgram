//! Emoji set for the panel: categories as in Telegram Desktop, plus quick reactions.

/// Reactions available in most chats (Telegram's standard ones).
pub const QUICK_REACTIONS: [&str; 10] = ["👍", "❤", "🔥", "😂", "😮", "😢", "🎉", "🙏", "👏", "👎"];

/// Telegram's standard reactions offered when a chat allows only some, in the official order.
pub const STANDARD_REACTIONS: [&str; 36] = [
    "👍", "👎", "❤", "🔥", "🥰", "👏", "😁", "🤔", "🤯", "😱", "🤬", "😢", "🎉", "🤩", "🤮", "💩", "🙏", "👌",
    "🤡", "🥱", "😍", "🐳", "💯", "🤣", "🍌", "🏆", "💔", "🤨", "😐", "🍓", "🍾", "💋", "😈", "😴", "😭", "👀",
];

use crate::i18n::l;

/// (title, emoji): the title is a function so it follows the UI language.
pub const CATEGORIES: [(fn() -> &'static str, &str); 7] = [
    (
        || l!("Смайли", "Smileys"),
        "😀😃😄😁😆😅🤣😂🙂🙃😉😊😇🥰😍🤩😘😗😚😙😋😛😜🤪😝🤑🤗🤭🤫🤔🤐🤨😐😑😶😏😒🙄😬🤥😌😔😪🤤😴😷🤒🤕🤢🤮🤧🥵🥶🥴😵🤯🤠🥳😎🤓🧐😕😟🙁😮😯😲😳🥺😦😧😨😰😥😢😭😱😖😣😞😓😩😫🥱😤😡😠🤬😈👿💀💩🤡👻👽🤖",
    ),
    (
        || l!("Жести", "Gestures"),
        "👋🤚🖐✋🖖👌🤏✌🤞🤟🤘🤙👈👉👆🖕👇☝👍👎✊👊🤛🤜👏🙌👐🤲🤝🙏✍💅🤳💪🦾🦵🦶👂🦻👃🧠🦷🦴👀👁👅👄💋",
    ),
    (
        || l!("Серця", "Hearts"),
        "❤🧡💛💚💙💜🖤🤍🤎💔❣💕💞💓💗💖💘💝💟☮✝☯🕉☸✡🔯🕎☦🛐⛎♈♉♊♋♌♍♎♏♐♑♒♓🆔⚛",
    ),
    (
        || l!("Тварини", "Animals"),
        "🐶🐱🐭🐹🐰🦊🐻🐼🐨🐯🦁🐮🐷🐸🐵🙈🙉🙊🐒🐔🐧🐦🐤🦆🦅🦉🦇🐺🐗🐴🦄🐝🐛🦋🐌🐞🐜🦗🕷🦂🐢🐍🦎🐙🦑🦐🦀🐡🐠🐟🐬🐳🐋🦈🐊🐅🐆🦓🦍🐘🦛🦏🐪🐫🦒🦘🐃🐂🐄🐎🐖🐏🐑🐐🦌🐕🐩🐈🐓🦃🦚🦜🦢🕊🐇🦝🦨🦡🦦🦥🐁🐀🐿🦔🌵🎄🌲🌳🌴🌱🌿☘🍀🍁🍂🍃🌷🌹🥀🌺🌸🌼🌻🌞🌝🌚🌙⭐🌟✨⚡🔥🌈☀⛅☁🌧⛈🌩❄☃⛄💧💦☔🌊",
    ),
    (
        || l!("Їжа", "Food"),
        "🍏🍎🍐🍊🍋🍌🍉🍇🍓🍈🍒🍑🥭🍍🥥🥝🍅🍆🥑🥦🥬🥒🌶🌽🥕🧄🧅🥔🍠🥐🥯🍞🥖🥨🧀🥚🍳🥞🧇🥓🥩🍗🍖🌭🍔🍟🍕🥪🥙🧆🌮🌯🥗🥘🍝🍜🍲🍛🍣🍱🥟🍤🍙🍚🍘🍥🥮🍢🍡🍧🍨🍦🥧🧁🍰🎂🍮🍭🍬🍫🍿🍩🍪🌰🥜🍯🥛🍼☕🍵🧃🥤🍶🍺🍻🥂🍷🥃🍸🍹🧉🍾",
    ),
    (
        || l!("Активності", "Activities"),
        "⚽🏀🏈⚾🥎🎾🏐🏉🥏🎱🏓🏸🏒🏑🥍🏏🥅⛳🏹🎣🥊🥋🎽🛹🛷⛸🥌🎿⛷🏂🏆🥇🥈🥉🏅🎖🎗🎫🎟🎪🎭🎨🎬🎤🎧🎼🎹🥁🎷🎺🎸🎻🎲♟🎯🎳🎮🎰🧩🚗🚕🚙🚌🚎🏎🚓🚑🚒🚐🚚🚛🚜🛴🚲🛵🏍✈🚀🛸🚁⛵🚤🛳⛴🚢⚓🗺🏔⛰🌋🏕🏖🏜🏝🏞🏟🏛🏗🏘🏠🏡🏢🏥🏦🏨🏪🏫🏰🗼🗽⛪🕌⛩🎡🎢🎠⛲🌅🌄🌠🎇🎆🌇🌆🏙🌃🌌🌉🌁",
    ),
    (
        || l!("Предмети", "Objects"),
        "⌚📱💻⌨🖥🖨🖱💽💾💿📀📷📸📹🎥📞☎📟📠📺📻🎙⏰⌛⏳📡🔋🔌💡🔦🕯💸💵💴💶💷💰💳💎⚖🧰🔧🔨⚒🛠⛏🔩⚙🧱⛓🧲🔫💣🧨🔪🗡⚔🛡🚬⚰🏺🔮📿🧿💈⚗🔭🔬🕳💊💉🧬🦠🧫🧪🌡🧹🧺🧻🚽🚰🚿🛁🧼🧽🧴🛎🔑🗝🚪🪑🛋🛏🧸🖼🛍🛒🎁🎈🎏🎀🎊🎉🎎🏮🎐🧧✉📩📨📧💌📥📤📦🏷📪📫📬📭📮📯📜📃📄📑🧾📊📈📉🗒🗓📆📅🗑📇🗃🗳🗄📋📁📂🗂🗞📰📓📔📒📕📗📘📙📚📖🔖🧷🔗📎🖇📐📏🧮📌📍✂🖊🖋✒🖌🖍📝✏🔍🔎🔏🔐🔒🔓",
    ),
];

/// Category emoji as separate strings (every item in the list is a single character).
pub fn list(chars: &str) -> Vec<String> {
    chars.chars().map(String::from).collect()
}
