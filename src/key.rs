use rand::prelude::*;

const EFF_DICE_LIST: &str = include_str!("./static/eff_large_wordlist.txt");

pub fn get_random_key(word_count: u8) -> String {
    let mut str = "".to_string();
    let list: Vec<&str> = EFF_DICE_LIST.lines().collect();

    let mut rng = rand::rng();

    // TODO: there are better ways to do this
    for _ in 0..word_count {
        let word_index: usize = rng.random_range(0..list.len());
        if !str.is_empty() {
            str += " ";
        }

        let mut filtered = String::from(list[word_index]).to_lowercase();
        filtered.retain(|c| c.is_alphabetic());

        str += filtered.as_str()
    }

    str
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_get_random_key() {
        for i in 1..20 {
            let key = super::get_random_key(i);
            let words: Vec<&str> = key.split(" ").collect();
            assert_eq!(words.len(), i as usize);
            for word in words {
                assert!(word.chars().all(char::is_alphabetic), "other than alpha");
            }
        }
    }
}
