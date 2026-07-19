//! Askamaテンプレート構造体。`templates/layout.html`を継承する各テンプレートは
//! `current_user`フィールドを持つ必要がある。

pub struct CurrentUser {
    pub display_name: String,
}
