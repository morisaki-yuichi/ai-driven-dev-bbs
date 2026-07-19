-- 初期スキーマ。DDLのみ・シードデータなし(H-08 / H-11、foundation-plan.md §4.1)。
-- 主キー型・URLのID形式・インデックス方針は decision 0019(D02、提案・未承認)による。

create table users (
    id bigint generated always as identity primary key,
    unique_id text not null unique,
    password_hash text not null,
    display_name text not null
);

create table threads (
    id bigint generated always as identity primary key,
    author_id bigint not null references users (id),
    title text not null,
    body text not null,
    created_at timestamptz(3) not null default now()
);

create table comments (
    id bigint generated always as identity primary key,
    thread_id bigint not null references threads (id),
    author_id bigint not null references users (id),
    body text not null,
    created_at timestamptz(3) not null default now(),
    -- 論理削除(C-07)。NULLでない = 削除済み。本文(body)は削除後も保持する。
    deleted_at timestamptz(3)
);

create index comments_thread_id_idx on comments (thread_id);

create table sessions (
    -- CSPRNG生成のランダムトークン。他の主キーと異なり連番にしない
    -- (foundation-plan.md §1.6、decision 0019)。
    id text primary key,
    user_id bigint not null references users (id)
);
