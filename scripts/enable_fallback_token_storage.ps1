# Enable fallback token storage for sempal
# This sets user-level environment variables permanently

$secret = "sempal-github-token-encryption-key-$(Get-Random -Minimum 100000 -Maximum 999999)"

[System.Environment]::SetEnvironmentVariable("SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE", "1", "User")
[System.Environment]::SetEnvironmentVariable("SEMPAL_FALLBACK_TOKEN_SECRET", $secret, "User")

Write-Host "✓ Fallback token storage enabled permanently for your user account"
Write-Host "✓ SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE = 1"
Write-Host "✓ SEMPAL_FALLBACK_TOKEN_SECRET = (set to random secret)"
Write-Host ""
Write-Host "Note: You may need to restart sempal for these changes to take effect."
