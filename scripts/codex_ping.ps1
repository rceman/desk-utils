$ErrorActionPreference = 'SilentlyContinue'
$log = "$env:USERPROFILE\codex_ping.log"
$j = Get-Content "$env:USERPROFILE\.codex\auth.json" -Raw | ConvertFrom-Json
$t = $j.tokens.access_token; $acc = $j.tokens.account_id
$body = '{"model":"gpt-6-luna","instructions":"reply pong","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"ping"}]}],"stream":true,"store":false}'
$bf = "$env:TEMP\cx_body.json"; $hf = "$env:TEMP\cx_hdr.txt"
$body | Out-File -Encoding ascii $bf
$code = curl.exe -s -o NUL -D $hf -w '%{http_code}' -X POST 'https://chatgpt.com/backend-api/codex/responses' -H "Authorization: Bearer $t" -H "chatgpt-account-id: $acc" -H 'OpenAI-Beta: responses=experimental' -H 'originator: codex_cli_rs' -H 'Content-Type: application/json' --data-binary "@$bf"
$p = (Select-String -Path $hf -Pattern 'x-codex-primary-used-percent:\s*([\d.]+)').Matches.Groups[1].Value
$s = (Select-String -Path $hf -Pattern 'x-codex-secondary-used-percent:\s*([\d.]+)').Matches.Groups[1].Value
$em = ''
try {
  $p2 = $j.tokens.id_token.Split('.')[1]; $p2 = $p2.Replace('-','+').Replace('_','/'); while ($p2.Length % 4) { $p2 += '=' }
  $em = ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($p2)) | ConvertFrom-Json).email
} catch {}
$lp = ($em -split '@')[0]
$acct = $lp.Substring(0, [Math]::Min(4, $lp.Length))
$pl = (Select-String -Path $hf -Pattern 'x-codex-plan-type:\s*(\w+)').Matches.Groups[1].Value
$r = (Select-String -Path $hf -Pattern 'x-codex-primary-reset-after-seconds:\s*(\d+)').Matches.Groups[1].Value
$now = [int][double]::Parse((Get-Date -UFormat %s))
if ($p) {
  "{`"primary_used`":$p,`"secondary_used`":$s,`"primary_reset`":$($now + [int]$r),`"plan`":`"$pl`",`"acct`":`"$acct`",`"ts`":$now}" | Set-Content "$env:USERPROFILE\.codex\limits_cache.json"
  Add-Content $log "$(Get-Date -Format 'HH:mm:ss') http=$code primary=$p secondary=$s reset_in=$r"
} else { Add-Content $log "$(Get-Date -Format 'HH:mm:ss') http=$code NO HEADERS" }
