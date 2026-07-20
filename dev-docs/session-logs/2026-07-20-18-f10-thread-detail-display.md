# セッションログ: 2026-07-20 #18 F10 スレッド詳細表示

- フェーズ: 開発（フェーズ4 実装、F10 スレッド詳細表示。branch `feat/thread-detail-display`、分岐元 `main` の `a7e8884`）
- 今回やったこと:
    - オーケストレーション（S1 状態復元 → G1 → S2 証明+TDD実装 → S3 レビュー → G3 裁定 → 裁定反映 → 追加裁定の反映）でF10を実装した。
    - G1でユーザーが承認したスコープはF10を表示系に絞る形: 未ログインリダイレクト、本文・作成者・作成日時の表示、コメント一覧、削除済みコメントの固定文言、404、一覧からのリンク配線。issue 10のACに含まれる「自分のスレッドに削除ボタン（コメント0件の場合のみ）」「自分のコメントに削除ボタン」はF06・F08の範囲として切り出した（F09でソート切替UIをF12へ切り出したのと同じ扱い）。
    - あわせて、長く持ち越されていた「ログイン中ユーザーが404を踏むと未ログイン用ヘッダーが出る」問題を今回修正する方針とし、修正方式は「ハンドラごとに直接404描画」ではなく「エラー経路が認証情報を運べるようにする」を採用した。
    - S2でLean証明: `deleted_comment_renders_fixed_text`（C-01/AC08-2）と`deleted_comment_keeps_metadata`（AC10-3）をsorryから実証明に置換した。今回は仮定不足は見つからなかった（過去3件とは異なる）。補題`insertBy_mem`・`sortBy_mem`を追加した（`formal/Bbs/Invariant.lean`、`formal/Bbs/Query.lean`）。
    - S3レビューを実施。重大は0件。ただし以下が判明した。
        1. 今回の修正で導入した`with_current_user`はopt-in設計で付け忘れを型で検出できず、F06・F07・F08で同種不具合が黙って再発しうる。
        2. fallback（未知URL）が`require_auth`の外にあるため、ログイン中に存在しないURLを踏むと未ログイン用ヘッダーが出る不具合が既に存在していた（今回のサイクルが直そうとした不具合と同一クラスが別経路に残っていた）。
        3. 成功経路と失敗経路で`current_user`を二重に受け渡していた。
    - S3の検証は、C-01固定文言をレンダリング済みHTMLからhexdumpし`ui-ux-guidelines.md`§8とバイト単位で照合、`/threads/{id}`に非数値・0・負数・26桁・`i64::MAX`・`i64::MAX+1`等を与えて全て404でpanicしないことを実測、未証明の`hit_is_reachable`を手検査で真であることを確認（`Wf`のid一意性すら不要）という内容だった。
- 決めたこと（関連 decision 番号があれば併記）:
    - G3でユーザーが裁定した内容を構造化し、`dev-docs/decisions/0028-error-page-auth-reflection.md`として記録・ユーザー承認済み（decision 0028）。採用した方式は、`AppError`から`current_user`と`with_current_user`を削除し、404レスポンスに`AuthAwareErrorPage`マーカーを載せ、ルータ全体に掛けたミドルウェアがマーカーを見つけたときだけセッションを解決して本文を描き直す形。ハンドラは`?`でNotFoundを返すだけでよく、付け忘れという状態が存在しない。CSRFの2ミドルウェアはこれより外側にあり、CSRFエラーにはマーカーが付かない（エラーページがログアウトフォームを描画してはならないという不変条件を二重に担保）。
    - 裁定反映の一部として、本文の`white-space: pre-wrap`指定、`error.rs`のWhy-notコメント復活、`formal/Bbs/Query.lean`のdoc訂正（コメントのページネーションはdecision 0013 §3で決定済み）を実施した。
    - 裁定反映後の検証で、今回の修正で404ページが初めて表示名を含むようになったため、fallbackの404に`Cache-Control: no-store`が付いていないことが意味を持つようになった（ログアウト後のブラウザバックでキャッシュ経由に認証済みヘッダーが見え得る。C-11/AC03-2）と判明し、追加で裁定・修正した。ミドルウェアが描き直した応答に`no-store`をinsertで付与（appendでないためヘッダーが二重にならない）し、回帰テストを追加した。
    - 裁定で見送りが確定した事項: `.thread-detail` / `.comment` / `.empty-state` / `.thread-card`のCSS定義（F09から続く既存ギャップ）、詳細表示の2クエリをトランザクションに入れる件（読み取り専用・影響軽微）、`overflow-wrap`の追加。
- 次にやること:
    - F06〜F08着手時、`comment_body_immutable`と`deletion_irreversible`について反例検査を必ず通す。
    - `comment_bumps_lastUpdated`の本体はsorryのままでF07実装時に埋める。その際`Wf`への時計支配フィールドの集約も行う。
    - `thread_immutable`の本体もsorryのままでF06〜F08で埋める。
    - ページリンクが`q`・`sort`を落とす件はF11・F12で必ず拾う。
    - 未コミット。コミットとPRはこの後の別ステップで行う。
- 未解決事項:
    - `comment_body_immutable`と`deletion_irreversible`は「Step列を跨ぐ横断的性質」という、過去に偽と判明した3件と同じ形をしており仮定不足の疑いが濃い。F06〜F08着手時に必ず反例検査を通すこと。
    - `comment_bumps_lastUpdated`の本体はsorryのまま（F07実装時に埋める予定）。
    - `thread_immutable`の本体もsorryのまま（F06〜F08で埋める予定）。
    - ページリンクが`q`・`sort`を落とす件（F11・F12で拾う予定）。
    - 必須フィールド欠落POSTが403でなく422になる。
    - 二重送信抑止はF07で再浮上見込み。
    - 未ログインで`/nosuchpage`を踏んだ応答には`no-store`が付かないままである（今回の裁定はこの経路を対象外とした）。
