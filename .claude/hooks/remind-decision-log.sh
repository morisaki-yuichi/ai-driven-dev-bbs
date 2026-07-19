#!/usr/bin/env bash
# Stopフック: 実装ソースに変更がある回だけ、decision記録をソフトに促す。
# 恒久ブロックにしない（continueを常にtrueのまま・systemMessageのみ）。
#
# 抑止条件は成果物ベース（dev-docs/decisions/ への変更の有無）にしている。
# 当初はセッションのtranscriptを '/log-decision' や「記録不要」という文字列で
# grepしていたが、この文字列はフック自身のsystemMessageやCLAUDE.mdの記述内にも
# 現れるため、一度リマインダが出ると以後そのセッションで恒常的に沈黙する欠陥が
# あった（フェーズ2レビュー 重大-3）。文字列照合をやめ、dev-docs/decisions/ の
# 実際の変更有無で判定する。
set -uo pipefail

INPUT="$(cat)"

# stop_hook_active: このフック自身が引き起こしたStopの再入なら、多重発火を避けて即終了する。
STOP_ACTIVE="$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo false)"
if [ "$STOP_ACTIVE" = "true" ]; then
  exit 0
fi

ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
cd "$ROOT" 2>/dev/null || exit 0

# 実装ソース(app/src)に変更が無ければ何もしない。
if ! git status --porcelain -- app/src 2>/dev/null | grep -q .; then
  exit 0
fi

# dev-docs/decisions/ に変更（新規ファイル・README索引の更新など）があれば、
# このセッションで decision 記録が行われたとみなし何もしない。
if git status --porcelain -- dev-docs/decisions 2>/dev/null | grep -q .; then
  exit 0
fi

echo '{"systemMessage": "実装ソース(app/src)に変更がありますが、dev-docs/decisions/ には変更が見当たりません。仕様の未規定点を解釈・決定した場合は /log-decision で記録してください。記録不要と判断した場合はこのメッセージは無視して構いません。"}'
exit 0
