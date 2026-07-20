# セッションログ: 2026-07-20 #20 F08 コメント削除

- フェーズ: 4（実装基盤構築後の機能実装フェーズ、F08）
- 今回やったこと:
  - 進行はオーケストレーション（S1 状態復元 → G1 → S2 証明+TDD実装 → S3 レビュー → G3 裁定 → 裁定反映）で行った。ブランチは `feat/comment-deletion`（分岐元 `main` の `74a70f1`）。
  - **S1（状態復元）**: `docs/product/issues/08_comment_deletion.md`・`dev-docs/requirements-analysis.md`・`dev-docs/harness-invariants.md`・`dev-docs/ui-ux-guidelines.md`・既存のF06〜F07実装（`app/src/web/thread_detail.rs`、`app/src/db/comments.rs`、`app/tests/comment_create_test.rs`）・decision 0002/0017/0021/0024/0025 を読み、実装方針を確認した。
  - **G1（ユーザー承認）**: 以下のスコープで承認を得た。
    - `deletion_irreversible` と `comment_body_immutable` の一般形をF08で閉じる。
    - Wf 保存補題は「最小連鎖」に絞る（7補題×11conjunctのフルセットはスコープ外）。
    - `thread_immutable` の本体はF06のRust実装が必要なため今回は触れない。
    - D18（削除確認ダイアログ）は「確認なしで即削除」とする。
    - Forbidden・AlreadyDeletedはハンドラで明示処理する。
  - **S2（証明+TDD実装）の成果**:
    - Lean形式化（Must、通常のRedより一段強い要求）: `formal/Bbs/Invariant.lean`に`findComment_noWriteOnSuccess`・`deleteComment_atomic`（decision 0002対応、原子性）を追加し証明した。`deletion_irreversible`（C-07/C-08、`sorry`だった一般形）を、`CommentDeletedExists`という存在命題の保存を`Step`8種すべてについて示し`runAll`へ帰納法で持ち上げる構成で証明した。`comment_body_immutable`（C-05、`sorry`だった一般形）を、`CommentTrackingInvariant`構造体（`nodup`/`fresh`/`cidBound`/`bodyMatch`の4フィールド）を`Step`8種すべてで保存されることを示し`runAll`へ帰納法で持ち上げる構成で証明した。`cidBound`（`c.id < db.nextCommentId`）は`hfresh`単体から導けない追加の局所仮定として要ることが証明作業中に判明した。`discard`（`register`/`login`/`createThread`/`createComment`を`Action Unit`へ包む）が`Functor.mapConst`の既定実装を通じて`Action.bind x (fun _ => Action.pure ())`へ`rfl`で reduce することを発見・活用し、`runStep`越しの証明を`unfold`+`simp only`+`cases`の直接展開で通した。`omega`タクティクがこのプロジェクトの`abbrev`ベースのID型（`CommentId`等）をNat演算として認識できない既知の制約を発見し、回避として`Nat.ne_of_lt`等の具体的な補題を直接使った。`lake build`は10件の`sorry`のみが残る状態で成功し、シナリオ煙試験（3/4/17/15/9件）も全てpassのまま。
    - Rust実装（TDD、Action/Calculation/Data分離）: `app/src/db/comments.rs`に`CommentRow`への`id`・`author_id`追加、`CommentOwnership`（`find_ownership`）・`delete`（論理削除）を新設。DBレベルの単体テスト6件を追加。`app/src/web/params.rs`に`DeleteCommentForm`（CSRFトークンのみ）を追加。`app/src/web/thread_detail.rs`に`CommentItem`への`id`・`can_delete`追加、`CommentDeleteNotice`（成功/forbidden/alreadyDeletedのフラッシュ通知、decision 0024と同じクエリパラメータ方式）を新設、`delete_comment`ハンドラを追加（`formal/Bbs/Op.lean`の`deleteComment`と同じ順序: requireAuth→findComment(404)→作成者検査(forbidden)→未削除検査(alreadyDeleted)→論理削除）。`render_detail`の引数増加をclippy指摘（too_many_arguments）で`CommentFormState`構造体へ整理した。`app/src/web/mod.rs`に`POST /threads/{thread_id}/comments/{comment_id}/delete`を`require_auth`配下で登録。`app/templates/thread_detail.html`に削除ボタン（自分の・未削除のコメントのみ、AC08-3/AC08-4/ui-ux-guidelines §1）、削除結果のフラッシュ通知エリア（login.htmlと同じaria-live使い分け）を追加。`app/tests/comment_deletion_test.rs`を新設しAC08-1〜AC08-4・C-07・C-09・C-10（存在しない/他スレッドのコメント）・CSRF・削除ボタンの表示可否・D18（confirm不使用）を結合テスト11件でカバーした。`app/tests/comment_create_test.rs`の`thread_detail_has_no_edit_ui_for_posted_comment`をF08で削除ボタンが正当に増えたことに合わせて「編集用の手段が無い」という本来の意図に絞る形へ更新した。
    - D18を decision 0030 として記録した。`cargo sqlx prepare -- --all-targets`実行、`.sqlx/`を更新。`cargo test`（169件）・`cargo clippy --all-targets`・`cargo fmt --check`が全て緑/クリーンであることを確認。`/verify`でホストの`cargo run`+Postgres実接続+curlにより、他人のコメント削除がForbiddenで拒否、本人の削除が成功、削除後のボタン非表示、二重削除がAlreadyDeleted、CSRF不一致で403、未ログインで/loginへリダイレクト、存在しないコメントIDで404を実レスポンス・DB状態で確認した。
    - **S2はG1承認済みスコープのうち「Wf保存補題の最小連鎖」に着手しておらず、報告にもその欠落の記載が無かった。この取りこぼしはS3のレビューで初めて確認された。**
  - **S3（レビュー）**: 重大な指摘は0件。特筆すべき検証として、新設した2つの不変条件（`CommentDeletedExists`・`CommentTrackingInvariant`）が「証明を通すためだけの実質的内容の無いもの」でないかを、`Op.lean`の`deleteComment`を意図的に壊す変異テストで実証した。変異①「削除時に本文も消す」→ tracking invariant の保存が失敗。変異②「物理削除（要素を除去）」→ tracking と `commentDeletedExists` の両方が失敗。検証後 `Op.lean` は復元済み。`#print axioms`で3定理（`deleteComment_atomic`・`deletion_irreversible`・`comment_body_immutable`）とも`sorryAx`不在を確認した。Rust側は、所有者判定がサーバ側で検証されボタン非表示に依存しないこと、別スレッドのコメントIDへのアクセスは404になること、IDOR・オープンリダイレクトの余地が無いことを実測確認した。D13/C-16（削除済みもコメント数に算入）は実削除経路で`comment_count`が削除前後とも2であることを実測確認した。
    - S3が指摘した中の指摘: `find_ownership`→`delete`間のTOCTOU（同時削除が双方とも`!deleted`検査を通過しうる）。既存の`sorry`である`deleteThread_blocked_by_deleted_comment`がAC06-2を全く表現していない（仮定が完全に未使用で、結論は`NoWriteOnError`の特殊化にすぎない。命題自体は真だが無意味）。
  - **G3（裁定）でユーザーが下し、以下の反映を行った**:
    1. Wf保存補題の最小連鎖を実施した。`Wf`に`nextUserIdFresh`・`nextCommentIdFresh`を追加（`userIdsDistinct`・`commentIdsDistinct`は他フィールドだけでは保存を証明できないため。decision 0027が`clockDominates*`を追加したのと同型の強化）。`register`・`login`・`createThread`・`createComment`の4保存補題（`register_preserves_wf`等）を証明し、`Db.empty`から到達した具体状態について`Wf`を保存補題の連鎖だけから導出し、`comment_bumps_lastUpdated_is_applicable`が通ることを確認した。「証明は通るが適用できない」状態が解消された。残件（`deleteThread`・`deleteComment`・`updateDisplayName`の保存補題、`runAll`への一般形）はdocコメントに明記した。
    2. TOCTOUを`update comments set deleted_at = now() where id = $1 and deleted_at is null`+`rows_affected`判定に修正した（`app/src/db/comments.rs`の`delete`）。**修正前のTOCTOUが実際に到達可能であることをレース再現で確認した**（別トランザクションが先に削除して行ロックを保持した状態でHTTP削除を発射すると、事前検査（`find_ownership`）が未削除を見て通過したあとUPDATEでロック待ちに入る経路が実在し、実測5.03秒ブロックしたのちロック解放でrows_affected=0となった）。判定の真偽の決定権を`delete`の戻り値（`bool`）に一本化し、`find_ownership`は早期リターンのための最適化に格下げした。並行削除の結合テスト（`concurrent_deletes_report_success_only_once`）を追加した。
    3. `deleteThread_blocked_by_deleted_comment`をAC06-2を実際に表現する形に restate し、F06送りにせずその場で証明まで完了した（`sorry` 10→9）。認可（作成者であること）を仮定に加えて結論を`.threadHasComments`に改め、`hcd`（削除済みであること）が証明に不要であること自体がAC06-2の内容であるという構成に整理し、一般形`deleteThread_blocked_by_any_comment`の特殊化として残した。
    4. 軽微3件（docコメントの誤記述、脆いテスト2件）を修正した。
  - 裁定で見送りが確定した事項: クエリパラメータ方式のフラッシュメッセージの見直し（decision 0024由来の既存パターン。誰でも`?comment_deleted=1`を叩けば成功文言を出せる点を含む）。`message-area`が常時`role="alert" aria-live="assertive"`で描画される非対称。いずれも既存パターンの踏襲として今回は見送り、決定の変更はしていない。
- 決めたこと（関連 decision 番号があれば併記）:
  - D18（削除確認ダイアログ）は「確認なしで即削除する」とした。`window.confirm`はH-02（agent-browser操作性）に悪影響がありdecision 0008（JSなし）とも整合しない。論理削除で固定文言が残るため完全な不可逆ではなく、誤操作の実害が限定的であることを理由とした（decision 0030）。
  - Forbidden・AlreadyDeletedは`web/error.rs`の一律400フォールバック（ハンドラの実装漏れの安全網）に頼らず、`delete_comment`ハンドラが明示的に捕捉して`/threads/{id}`へのフラッシュ付きリダイレクトを返す方式にした（ユーザー承認済みのスコープ、decision化はしていない実装判断）。
  - URLの`thread_id`セグメントが実際のコメントの所属スレッドと食い違う場合はC-10の404として扱う（ネスト構造の整合性、明示のdecisionは起票していない軽微な実装判断）。
  - G3裁定によりTOCTOU対策の方式を確定した: 「未削除であることの確認」と「削除」を`where`句+`rows_affected`の1文=1原子操作にまとめる。F06（スレッド削除）も同じ形（`delete from threads where id = $1 and not exists (コメント)`）を踏襲できることをdocコメントに明記した（decision化はしていない、実装パターンとしての申し送り）。
  - Wf拡張（`nextUserIdFresh`・`nextCommentIdFresh`の追加）はdecision 0027が確立した「Wfを帰納的にするために必要な強化を末尾フィールドとして追加する」パターンの踏襲であり、新規decisionは起票していない。
- 次にやること:
  - `/code-review`・`/security-review`（本サイクルでは実施しない、次のステップで別途実行予定）。
  - `git add`・`git commit`・PR作成はユーザー承認後に別途行う（本サイクルでは未実施、指示どおり）。
  - `dev-docs/decisions/README.md`の`decided_by = ai`索引の棚卸しは今回0件のため不要（decision 0030はユーザー承認済みの`ai+user`として起票済み）。
- 未解決事項:
  - `Wf.nextIdsFresh`が`threads`にしか定義がなく`comments`/`users`/`sessions`の同種プロパティが無い、という副次的なギャップは、`comment_body_immutable`の証明では`CommentTrackingInvariant`という局所的な代替構造（`nodup`/`fresh`/`cidBound`）で回避していたが、G3の裁定反映で`nextUserIdFresh`・`nextCommentIdFresh`を`Wf`本体に追加したことで一部解消した。
  - Wf保存補題の残件: `deleteThread`（F06、物理削除で`commentThreadsExist`の維持が論点）・`deleteComment`（F08）・`updateDisplayName`（F04）の保存補題、および`Wf`を`runAll`（`Step`列）へ持ち上げた一般形。`formal/Bbs/Invariant.lean`のセクション1.1冒頭docコメントに具体的な残作業として明記済み。
  - `omega`が`abbrev`型のID（`CommentId`等）を認識できない制約は、今後の証明セッションでも同様の回避（具体的なNat補題を直接使う）が必要になる可能性がある。原因（Lean 4.32.0の既知動作か本プロジェクト固有の設定起因か）は未調査のまま。
  - `thread_immutable`の本体はF06のRust実装が必要。`deleteThread_atomic`・`deleteThread_needs_owner`も同様にF06。
  - `displayName_propagates`はF04送り。
  - `search_finds_body`・`no_deleted_hit`・`hit_is_reachable`はF11送り。
  - `sorted_by_commentCount`・`createdAsc_head_is_oldest`はF12送り。
  - ページリンクが`q`・`sort`パラメータを落とす件は、F11・F12の実装時に拾う。
  - 必須フィールド欠落POSTが403（CSRF検証相当）でなく422になる件は未解決のまま。
  - 未ログインで未知URLを踏んだ応答には`no-store`が付かない件は未解決のまま。
  - `db::comments::delete`の戻り値を`bool`に変えたことで、F06のスレッド削除も「条件付き1文 + `rows_affected`」を踏襲できる（`app/src/db/comments.rs`のdocコメントに具体的なSQL形まで記載済み）。
  - `deleteThread_blocked_by_deleted_comment`はAC06-2を表現する形に修正・証明済みだが、`deleteThread_atomic`・`thread_immutable`・`deleteThread_needs_owner`は引き続き`sorry`（F06本体待ち）。
