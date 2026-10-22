//! Normalized events from Bilibili web command packets.
use serde_json::Value;

pub use nanatype_chat::{Author, ChatEvent, Guard, int, millis, string, url};

/// Accept both positional web messages and the richer user object in newer messages.
pub fn parse(frame: &Value) -> Option<ChatEvent> {
    let cmd = frame["cmd"].as_str()?.split(':').next()?;
    let d = &frame["data"];
    match cmd {
        "DANMU_MSG" => {
            let i = &frame["info"];
            let meta = &i[0][15];
            let extra: Value = meta["extra"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let u = if meta["user"].is_object() {
                &meta["user"]
            } else {
                &extra["user"]
            };
            let name = string(&i[2][1]);
            if name.is_empty() && u["base"]["name"].as_str().is_none() {
                return None;
            }
            let author = Author {
                uid: if !u["uid"].is_null() {
                    string(&u["uid"])
                } else {
                    string(&i[2][0])
                },
                name: u["base"]["name"].as_str().unwrap_or(&name).into(),
                avatar_url: url(&u["base"]["face"]),
                admin: int(&i[2][2]) == 1,
                guard: Guard::from_privilege(int(&i[7])),
                medal_level: int(&i[3][0]).max(0) as u32,
                medal_name: string(&i[3][1]),
                ..Default::default()
            };
            let id = extra["id_str"]
                .as_str()
                .or_else(|| meta["id_str"].as_str())
                .unwrap_or_default()
                .into();
            let emote = url(&i[0][13]["url"]);
            Some(ChatEvent::Text {
                id,
                author,
                content: string(&i[1]),
                emoticon: (!emote.is_empty()).then_some(emote),
                at_ms: millis(&i[0][4]),
            })
        }
        "SEND_GIFT" => Some(ChatEvent::Gift {
            id: string(&d["tid"]),
            author: object_author(d),
            gift_name: string(&d["giftName"]),
            num: int(&d["num"]).max(1) as u32,
            total_coin: if d["coin_type"] == "gold" {
                int(&d["total_coin"]).max(0) as u64
            } else {
                0
            },
            at_ms: millis(&d["timestamp"]),
        }),
        "GUARD_BUY" => Some(ChatEvent::Member {
            id: string(&d["id"]),
            author: object_author(d),
            num: int(&d["num"]).max(1) as u32,
            unit: "月".into(),
            total_coin: (int(&d["price"]).max(0) as u64)
                .saturating_mul(int(&d["num"]).max(1) as u64),
            at_ms: millis(&d["start_time"]),
        }),
        "SUPER_CHAT_MESSAGE" => {
            let mut author = object_author(d);
            author.name = string(&d["user_info"]["uname"]);
            author.avatar_url = url(&d["user_info"]["face"]);
            Some(ChatEvent::SuperChat {
                id: string(&d["id"]),
                author,
                price: int(&d["price"]).max(0) as u64,
                content: string(&d["message"]),
                at_ms: millis(&d["start_time"]),
            })
        }
        "SUPER_CHAT_MESSAGE_DELETE" => Some(ChatEvent::DeleteSuperChats {
            ids: d["ids"]
                .as_array()
                .map(|a| a.iter().map(string).collect())
                .unwrap_or_default(),
        }),
        _ => None,
    }
}
fn object_author(d: &Value) -> Author {
    Author {
        uid: string(&d["uid"]),
        name: d["uname"]
            .as_str()
            .or_else(|| d["username"].as_str())
            .unwrap_or_default()
            .into(),
        avatar_url: url(&d["face"]),
        guard: Guard::from_privilege(int(&d["guard_level"])),
        ..Default::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn web_chat_retains_uid_identity_and_artwork() {
        let frame = json!({"cmd":"DANMU_MSG:4:0", "info":[[0,1,25,0,1700000000000u64,0,0,0,0,0,0,0,0,{},0,{"extra":"{\"id_str\":\"same-as-receipt\"}","user":{"uid":7,"base":{"name":"viewer","face":"//i0.hdslb.com/a.jpg"}}}],"hello",[7,"viewer",1],[5,"medal"],0,0,0,3]});
        let Some(ChatEvent::Text {
            id,
            author,
            at_ms,
            content,
            ..
        }) = parse(&frame)
        else {
            panic!("chat missing")
        };
        assert_eq!(id, "same-as-receipt");
        assert_eq!(author.uid, "7");
        assert_eq!(author.name, "viewer");
        assert_eq!(author.avatar_url, "https://i0.hdslb.com/a.jpg");
        assert!(author.admin);
        assert_eq!(author.guard, Guard::Captain);
        assert_eq!(author.medal_level, 5);
        assert_eq!(content, "hello");
        assert_eq!(at_ms, 1700000000000);
    }
    #[test]
    fn paid_events_and_deletions_are_not_lost() {
        let gift=parse(&json!({"cmd":"SEND_GIFT","data":{"uid":9,"uname":"v","giftName":"gift","num":2,"total_coin":2000,"coin_type":"gold"}})).unwrap();
        assert!(matches!(
            gift,
            ChatEvent::Gift {
                total_coin: 2000,
                num: 2,
                ..
            }
        ));
        assert!(
            matches!(parse(&json!({"cmd":"GUARD_BUY","data":{"uid":9,"username":"captain","num":1,"price":198000,"guard_level":3}})),Some(ChatEvent::Member{author,..}) if author.name=="captain" && author.guard==Guard::Captain)
        );
        assert!(
            matches!(parse(&json!({"cmd":"SUPER_CHAT_MESSAGE","data":{"id":12,"uid":9,"user_info":{"uname":"v","face":"//x/a"},"message":"hi","price":30}})),Some(ChatEvent::SuperChat{price:30,author,..}) if author.uid=="9")
        );
        assert_eq!(
            parse(&json!({"cmd":"SUPER_CHAT_MESSAGE_DELETE","data":{"ids":[12,"13"]}})),
            Some(ChatEvent::DeleteSuperChats {
                ids: vec!["12".into(), "13".into()]
            })
        );
        assert!(parse(&json!({"cmd":"future"})).is_none());
        assert!(parse(&json!({"cmd":"DANMU_MSG","info":[]})).is_none());
    }
}
