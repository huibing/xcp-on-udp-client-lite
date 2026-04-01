# XCP Web UI 综合测试脚本
# 测试所有多从机功能

$baseUrl = "http://localhost:8080"
$cookieJar = "cookies.txt"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "XCP Web UI Multi-Slave Test Suite" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 清理之前的 cookie
Remove-Item $cookieJar -ErrorAction SilentlyContinue

function Test-Endpoint {
    param(
        [string]$Name,
        [string]$Method,
        [string]$Url,
        [hashtable]$FormData = $null,
        [switch]$SaveCookie,
        [switch]$UseCookie
    )

    Write-Host "`n[Test] $Name" -ForegroundColor Yellow
    Write-Host "  $Method $Url"

    try {
        $headers = @{}
        if ($UseCookie -and (Test-Path $cookieJar)) {
            $cookie = Get-Content $cookieJar -Raw
            $headers['Cookie'] = $cookie.Trim()
        }

        if ($FormData) {
            $body = $FormData | ForEach-Object { "$($_.Key)=$($_.Value)" } | Join-String -Separator '&'
            $response = Invoke-WebRequest -Uri $Url -Method $Method -Body $body -Headers $headers -ContentType "application/x-www-form-urlencoded" -UseBasicParsing
        } else {
            $response = Invoke-WebRequest -Uri $Url -Method $Method -Headers $headers -UseBasicParsing
        }

        if ($SaveCookie) {
            $response.Headers['Set-Cookie'] | Out-File $cookieJar
        }

        Write-Host "  Status: $($response.StatusCode) - OK" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "  Status: Failed - $_" -ForegroundColor Red
        return $false
    }
}

# 1. 测试登录页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 1: Authentication Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Login Page (GET)" -Method "GET" -Url "$baseUrl/login"

# 2. 测试登录 API
$loginForm = @{
    username = "admin"
    password = "admin"
}
Test-Endpoint -Name "Login API (POST)" -Method "POST" -Url "$baseUrl/api/login" -FormData $loginForm -SaveCookie

# 3. 测试 Dashboard（需要登录）
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 2: Dashboard Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Dashboard (authenticated)" -Method "GET" -Url "$baseUrl/" -UseCookie

# 4. 测试连接页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 3: Connection Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Connect Page" -Method "GET" -Url "$baseUrl/connect" -UseCookie

# 5. 测试连接到 Slave
Write-Host "`n[Test] Connect to Slave at localhost:5555" -ForegroundColor Yellow
$connectForm = @{
    name = "ECU1"
    address = "127.0.0.1:5555"
}
Test-Endpoint -Name "Connect API" -Method "POST" -Url "$baseUrl/api/connect" -FormData $connectForm -UseCookie

# 等待连接建立
Write-Host "  Waiting for connection..." -ForegroundColor Gray
Start-Sleep -Seconds 2

# 6. 测试 Dashboard 显示连接状态
Test-Endpoint -Name "Dashboard (after connect)" -Method "GET" -Url "$baseUrl/" -UseCookie

# 7. 测试 A2L 页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 4: A2L Management Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "A2L Page" -Method "GET" -Url "$baseUrl/a2l" -UseCookie

# 8. 加载 A2L 文件
$a2lForm = @{
    path = "hello_xcp.a2l"
}
Test-Endpoint -Name "Load A2L" -Method "POST" -Url "$baseUrl/api/a2l/load" -FormData $a2lForm -UseCookie

Start-Sleep -Seconds 1

# 9. 测试校准页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 5: Calibration Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Calibration Page" -Method "GET" -Url "$baseUrl/calibration" -UseCookie

# 10. 测试 DAQ 页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 6: DAQ Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "DAQ Page" -Method "GET" -Url "$baseUrl/daq" -UseCookie

# 11. 测试从机管理页面
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 7: Multi-Slave Management Tests" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Slaves List Page" -Method "GET" -Url "$baseUrl/slaves" -UseCookie

# 12. 测试断开连接
Write-Host "`n[Test] Disconnect from Slave" -ForegroundColor Yellow
Test-Endpoint -Name "Disconnect API" -Method "GET" -Url "$baseUrl/api/disconnect" -UseCookie

# 13. 测试登出
Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Phase 8: Logout Test" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

Test-Endpoint -Name "Logout" -Method "GET" -Url "$baseUrl/logout" -UseCookie

# 14. 验证登出后无法访问受保护页面
Write-Host "`n[Test] Verify protected page after logout" -ForegroundColor Yellow
$response = Invoke-WebRequest -Uri "$baseUrl/" -Method "GET" -UseBasicParsing
if ($response.Content -match "login" -or $response.StatusCode -eq 302) {
    Write-Host "  Status: Redirected to login - OK" -ForegroundColor Green
} else {
    Write-Host "  Status: Still accessible - FAILED" -ForegroundColor Red
}

Write-Host "`n==========================================" -ForegroundColor Cyan
Write-Host "Test Suite Complete!" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 清理
Remove-Item $cookieJar -ErrorAction SilentlyContinue
