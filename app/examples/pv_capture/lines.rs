//! What the trailer's footage says, per language. NanaPVMaker's
//! `projects/nanatype-steam/copy.ts` quotes the same strings.

use nanatype_i18n::Locale;

pub struct Lines {
    pub speaker: &'static str,
    /// The box's reveal speed (`playback.speed_scale`, lower is faster): the English and
    /// Japanese lines run longer than the Chinese, so they play faster to fit the same shot.
    pub speed: f32,
    /// Frames per typed character for the long typed lines (mirror, chat reply).
    pub type_every: u64,
    /// The hook: typed into the box over the game.
    pub claim: &'static str,
    /// Already in the history when the core demo starts.
    pub earlier: [&'static str; 2],
    /// Typed and sent in the core demo.
    pub line: &'static str,
    pub markup: &'static str,
    pub rhythm: &'static str,
    /// What speech recognition hears.
    pub voice: &'static str,
    /// Mirror mode: typed, the last `mirror_back` characters erased, then finished.
    pub mirror: &'static str,
    pub mirror_back: usize,
    pub mirror_rest: &'static str,
    pub sticker: &'static str,
    /// Typed into the input to open the `/` effect list.
    pub complete: &'static str,
    /// Telegram sticker search: the query, and the packs it finds (name, title, count).
    pub telegram: (&'static str, [(&'static str, &'static str, usize); 3]),
    /// Typed into the summoned input over a fullscreen game.
    pub keys: &'static str,
    pub skins: &'static str,
    pub workshop: [&'static str; 3],
    pub partners: [&'static str; 2],
    pub room_local: &'static str,
    pub room_partner: [&'static str; 2],
    /// Twitch viewers and what they write.
    pub chat: [(&'static str, &'static str); 3],
    pub reply: &'static str,
}

pub fn of(locale: Locale) -> &'static Lines {
    match locale {
        Locale::EnUs => &EN,
        Locale::JaJp => &JA,
        _ => &ZH,
    }
}

const ZH: Lines = Lines {
    speed: 1.0,
    type_every: 2,
    speaker: "奈奈",
    claim: "你打的每一句，都是台词！",
    earlier: ["欢迎来到直播间！", "今天换了新皮肤哦～"],
    line: "大家晚上好！今天也一起玩游戏吧～",
    markup: "[wave]波浪[/wave]、[shake]抖动[/shake]、[rainbow]彩虹[/rainbow]，还有[ruby=かんじ]漢字[/ruby]注音！",
    rhythm: "嗯……[wait=500]其实，[em]我有话[/em]想说[se=coin]！",
    voice: "说出来的话，也能变成台词",
    mirror: "边打字边显示错字",
    mirror_back: 2,
    mirror_rest: "，实时同步！",
    sticker: "今天也元气满满[sticker=nana/heart][sticker=nana/star]",
    complete: "这句要加点特效 /wav",
    telegram: (
        "猫",
        [
            ("nana_neko", "奈奈的猫猫日常", 24),
            ("neko_reactions", "猫猫表情反应", 40),
            ("cat_stamp_jp", "ねこスタンプ", 32),
        ],
    ),
    keys: "Boss 战开始了，大家帮我加油！",
    skins: "换一套皮肤，换一种心情～",
    workshop: ["樱花信笺", "复古街机", "像素音效包"],
    partners: ["小林", "澪"],
    room_local: "我们一起来配音吧！",
    room_partner: ["好呀，我来演反派！", "那旁白就交给我～"],
    chat: [
        ("pixel_cat", "主播晚上好！"),
        ("mochi", "今天玩什么游戏？"),
        ("kuro", "求自我介绍～"),
    ],
    reply: "今天玩新出的恐怖游戏哦！",
};

const EN: Lines = Lines {
    speed: 0.5,
    type_every: 1,
    speaker: "Nana",
    claim: "Every line you type becomes dialogue!",
    earlier: ["Welcome to the stream!", "New skin today~"],
    line: "Evening, chat! Let's play~",
    markup: "[wave]Waves[/wave], [shake]shakes[/shake], [rainbow]rainbows[/rainbow] and [ruby=kanji]漢字[/ruby] ruby text!",
    rhythm: "Well...[wait=500] actually, [em]I have[/em] something to say[se=coin]!",
    voice: "Say it out loud, and it becomes a line",
    mirror: "Shows up as you tpye",
    mirror_back: 4,
    mirror_rest: "type, live!",
    sticker: "Full of energy today[sticker=nana/heart][sticker=nana/star]",
    complete: "This line needs some flair /wav",
    telegram: (
        "cat",
        [
            ("nana_neko", "Nana's Cat Days", 24),
            ("neko_reactions", "Cat Reactions", 40),
            ("cat_stamp_jp", "Neko Stamps", 32),
        ],
    ),
    keys: "Boss fight! Cheer me on, chat!",
    skins: "New skin, new mood~",
    workshop: ["Sakura Letter", "Retro Arcade", "Pixel Sound Pack"],
    partners: ["Rin", "Mio"],
    room_local: "Let's do a voice drama together!",
    room_partner: ["Sure, I'll play the villain!", "And I'll narrate~"],
    chat: [
        ("pixel_cat", "Good evening!"),
        ("mochi", "What are we playing today?"),
        ("kuro", "Self-intro please~"),
    ],
    reply: "A brand-new horror game!",
};

const JA: Lines = Lines {
    speed: 0.7,
    type_every: 2,
    speaker: "ナナ",
    claim: "打った言葉が、ぜんぶセリフになる！",
    earlier: ["配信へようこそ！", "今日は新しいスキンだよ～"],
    line: "こんばんは！今日も一緒にゲームしよう～",
    markup: "[wave]なみなみ[/wave]、[shake]ぶるぶる[/shake]、[rainbow]にじいろ[/rainbow]、[ruby=かんじ]漢字[/ruby]のルビも！",
    rhythm: "えっと……[wait=500]じつは、[em]話したい[/em]ことが[se=coin]！",
    voice: "声に出した言葉も、そのままセリフに",
    mirror: "打ちながら表示されるもじ",
    mirror_back: 2,
    mirror_rest: "、リアルタイム！",
    sticker: "今日も元気いっぱい[sticker=nana/heart][sticker=nana/star]",
    complete: "このセリフに演出を /wav",
    telegram: (
        "ねこ",
        [
            ("nana_neko", "ナナのねこ日和", 24),
            ("neko_reactions", "ねこリアクション", 40),
            ("cat_stamp_jp", "ねこスタンプ", 32),
        ],
    ),
    keys: "ボス戦開始！みんな応援してね！",
    skins: "スキンを変えて、気分も変えて～",
    workshop: ["桜の便箋", "レトロアーケード", "ピクセル効果音パック"],
    partners: ["リン", "ミオ"],
    room_local: "一緒にボイスドラマやろう！",
    room_partner: ["いいよ、悪役やるね！", "じゃあナレーションは私～"],
    chat: [
        ("pixel_cat", "こんばんは！"),
        ("mochi", "今日は何のゲーム？"),
        ("kuro", "自己紹介して～"),
    ],
    reply: "新作のホラーゲームだよ！",
};
