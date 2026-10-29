//! What each skin's box says on the skin wall: a line in that skin's own voice.

use nanatype_i18n::Locale;

/// `(skin id without "nanatype.", [zh, en, ja])`
const LINES: &[(&str, [&str; 3])] = &[
    (
        "adv",
        [
            "那一天，雨下个不停。",
            "That day, the rain never stopped.",
            "あの日、雨は止まなかった。",
        ],
    ),
    (
        "bubble",
        [
            "今天也要甜甜的哦～",
            "Stay sweet today~",
            "今日もあまあまでいこう～",
        ],
    ),
    (
        "celestial",
        [
            "星星替我许了愿。",
            "The stars made my wish.",
            "星が願いを叶えてくれる。",
        ],
    ),
    (
        "citrus",
        [
            "来一口，夏天的味道！",
            "A sip of summer!",
            "ひと口で、夏の味！",
        ],
    ),
    (
        "comic",
        [
            "咚——！大事件发生了！",
            "BAM! Something big happened!",
            "ドーン！大事件だ！",
        ],
    ),
    (
        "crayon",
        [
            "今天画了一朵小花。",
            "I drew a little flower today.",
            "今日はお花を描いたよ。",
        ],
    ),
    (
        "echo_bubble",
        [
            "Echo-Live 老朋友，好久不见！",
            "Echo-Live, old friend!",
            "Echo-Live、久しぶり！",
        ],
    ),
    (
        "echo_glass",
        [
            "字幕浮在画面上。",
            "Words float over the scene.",
            "言葉が画面に浮かぶ。",
        ],
    ),
    (
        "echo_vanilla",
        [
            "最熟悉的那个样子。",
            "Just like you remember.",
            "いつものあの形。",
        ],
    ),
    (
        "echo_void",
        ["只留下文字。", "Only the words remain.", "文字だけが残る。"],
    ),
    (
        "forest",
        [
            "森林里有风的声音。",
            "The forest hums with wind.",
            "森に風の音がする。",
        ],
    ),
    (
        "gothic",
        [
            "钟声响起，夜开始了。",
            "The bells toll; night begins.",
            "鐘が鳴り、夜が始まる。",
        ],
    ),
    (
        "halo",
        [
            "大家好，我是奈奈！",
            "Hi everyone, I'm Nana!",
            "みんな、ナナだよ！",
        ],
    ),
    (
        "ink",
        [
            "一笔一画，皆是心意。",
            "Every stroke, from the heart.",
            "一筆ごとに、心を込めて。",
        ],
    ),
    (
        "lantern",
        [
            "灯笼亮了，今晚逛庙会！",
            "Lanterns lit, to the fair!",
            "提灯が灯る、お祭りへ！",
        ],
    ),
    (
        "livechat",
        [
            "弹幕刷起来！",
            "Chat, light it up!",
            "コメントいっぱいちょうだい！",
        ],
    ),
    (
        "love_letter",
        [
            "见字如面，展信佳。",
            "Dear you, I hope this finds you well.",
            "拝啓、お元気ですか。",
        ],
    ),
    (
        "machine_learning",
        [
            "正在学习你的说话方式……",
            "Learning how you talk...",
            "あなたの話し方を学習中……",
        ],
    ),
    (
        "night",
        [
            "霓虹下，营业中。",
            "Open late, under neon.",
            "ネオンの下、営業中。",
        ],
    ),
    (
        "parchment",
        [
            "勇者啊，翻开这一页吧。",
            "Hero, turn the page.",
            "勇者よ、頁をめくれ。",
        ],
    ),
    (
        "pixel",
        ["按 A 键继续。", "Press A to continue.", "Aボタンでつぎへ。"],
    ),
    (
        "rpg",
        [
            "史莱姆出现了！",
            "A slime appears!",
            "スライムがあらわれた！",
        ],
    ),
    (
        "sakura",
        [
            "樱花落下的速度。",
            "The speed of falling petals.",
            "桜の舞い散る速さ。",
        ],
    ),
    (
        "sonnet",
        [
            "愿这句诗化作光。",
            "May this verse turn to light.",
            "この詩よ、光になれ。",
        ],
    ),
    (
        "subtitle",
        [
            "（远处传来脚步声）",
            "(Footsteps in the distance)",
            "（遠くで足音がする）",
        ],
    ),
    (
        "terminal",
        [
            "$ ./start_stream --now",
            "$ ./start_stream --now",
            "$ ./start_stream --now",
        ],
    ),
    (
        "tidal",
        [
            "水母说，晚安。",
            "The jellyfish says goodnight.",
            "クラゲがおやすみって。",
        ],
    ),
    (
        "vanilla",
        [
            "今日特辑：打字的艺术。",
            "Today: the art of typing.",
            "本日の特集：タイプの技法。",
        ],
    ),
];

/// The skin wall's line for `skin_id` (falls back to `default`).
pub fn of(skin_id: &str, locale: Locale, default: &'static str) -> &'static str {
    let short = skin_id.trim_start_matches("nanatype.");
    let i = match locale {
        Locale::EnUs => 1,
        Locale::JaJp => 2,
        _ => 0,
    };
    LINES
        .iter()
        .find(|(id, _)| *id == short)
        .map_or(default, |(_, l)| l[i])
}
