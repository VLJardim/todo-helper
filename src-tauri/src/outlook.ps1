# Writes new Inbox mail from every account in the running classic Outlook to $Out as JSON:
# { accounts, mail }. Mail received after $Since (an ISO 8601 time) counts as new. Reads only; changes nothing.
param([string]$Since, [string]$Out)
$ErrorActionPreference = 'Stop'
try { $outlook = [Runtime.InteropServices.Marshal]::GetActiveObject('Outlook.Application') }
catch { [Console]::Error.WriteLine('OUTLOOK_NOT_RUNNING'); exit 3 }
$since = [DateTime]::Parse($Since, [Globalization.CultureInfo]::InvariantCulture, 'RoundtripKind').ToLocalTime()
$mail = New-Object System.Collections.ArrayList
$accounts = 0
foreach ($store in $outlook.Session.Stores) {
  # Archives, shared calendars and public folders have no Inbox.
  try { $items = $store.GetDefaultFolder(6).Items } catch { continue } # 6 = olFolderInbox
  $accounts++
  $items.Sort('[ReceivedTime]', $true)
  $taken = 0
  foreach ($m in $items) {
    if ($m.ReceivedTime -le $since) { break }
    if ($m.Class -ne 43) { continue } # 43 = olMail; skips meeting requests and delivery reports
    # The Message-ID stays the same when Outlook moves or re-syncs the mail; EntryID is the fallback.
    $id = try { $m.PropertyAccessor.GetProperty('http://schemas.microsoft.com/mapi/proptag/0x1035001F') } catch { '' }
    if (-not $id) { $id = $m.EntryID }
    $address = $m.SenderEmailAddress
    if ($m.SenderEmailType -eq 'EX') { try { $address = $m.Sender.GetExchangeUser().PrimarySmtpAddress } catch {} }
    $body = [string]$m.Body
    if ($body.Length -gt 8000) { $body = $body.Substring(0, 8000) }
    [void]$mail.Add([ordered]@{
      id = [string]$id
      sender = "$($m.SenderName) <$address>"
      subject = [string]$m.Subject
      received = $m.ReceivedTime.ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
      body = $body
    })
    # ponytail: 200 per account per check; a bigger backlog arrives over the next checks.
    if (++$taken -ge 200) { break }
  }
}
# A file, not stdout, so a big reply can't fill the pipe; UTF-8 without a BOM.
[IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject @{ accounts = $accounts; mail = @($mail) } -Compress -Depth 4), (New-Object Text.UTF8Encoding $false))
