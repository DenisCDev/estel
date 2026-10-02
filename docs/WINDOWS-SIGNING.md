# Assinatura para distribuição no Windows

O Estel ainda precisa de um certificado de assinatura de código de uma autoridade aceita pelo Windows, ou de um serviço de assinatura confiável contratado pelo responsável. Não há certificado nem serviço configurado neste projeto. SHA-256 garante integridade do arquivo; não substitui a identidade do editor. A documentação do [Smart App Control, atualizada em 28/09/2026](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control), aceita certificados RSA e ECC de provedores confiáveis. Não crie um certificado autoassinado nem importe uma raiz privada para tentar contornar essa exigência.

## Caminho de publicação preparado

O modo normal do instalador continua disponível para desenvolvimento. O modo explicitamente assinado é ativado com `/DEstelSignedBuild`; exige o comando Inno `estel_release`. Nesse modo o compilador assina o executável de origem antes de incorporá-lo, o instalador e o desinstalador. Um erro de assinatura interrompe a compilação, sem tentativas automáticas. Consulte [SignTool do Inno](https://jrsoftware.org/ishelp/topic_setup_signtool.htm), [SignedUninstaller](https://jrsoftware.org/ishelp/topic_setup_signeduninstaller.htm) e o [sinalizador sign](https://jrsoftware.org/ishelp/topic_filessection.htm).

Depois de obter o certificado, mantenha sua chave no armazenamento protegido indicado pelo emissor. O exemplo abaixo usa um certificado já disponível no armazenamento pessoal do usuário e o SignTool do Windows SDK. Substitua os caminhos, a impressão digital e a URL de carimbo de tempo pelos valores do ambiente e do emissor. Não coloque senha, PFX ou chave privada no repositório. Um serviço remoto exigirá o comando oficial fornecido pelo serviço contratado; este projeto não presume qual será ele.

```powershell
$env:ESTEL_VERSION = '0.2.9' # Use the version being published.
$signTool = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe'
$iscc = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
$certificateThumbprint = 'SUBSTITUA_PELA_IMPRESSAO_DIGITAL_DE_40_HEXADECIMAIS'
$timestampUrl = 'https://SUBSTITUA_PELO_SERVIDOR_DO_EMISSOR'
$signCommand = '$q' + $signTool + '$q sign /sha1 ' + $certificateThumbprint +
    ' /fd SHA256 /tr ' + $timestampUrl + ' /td SHA256 $f'
$start = [Diagnostics.ProcessStartInfo]::new($iscc)
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$start.ArgumentList.Add('/DEstelSignedBuild')
$start.ArgumentList.Add('/Sestel_release=' + $signCommand)
$start.ArgumentList.Add((Join-Path $PWD 'installer/estel.iss'))
$compiler = [Diagnostics.Process]::Start($start)
try {
    if (-not $compiler.WaitForExit(120000)) {
        $compiler.Kill($true)
        throw 'A criação e assinatura do instalador excederam 120 segundos.'
    }
    if ($compiler.ExitCode -ne 0) { throw 'O instalador assinado não foi concluído.' }
} finally { $compiler.Dispose() }

# Inno signed the source EXE; copy the portable executable only afterward.
Copy-Item -LiteralPath target/release/estel.exe -Destination artifacts/estel-portable-x86_64.exe
./scripts/Test-EstelSignatures.ps1 -SignToolPath $signTool `
    -ExpectedSignerThumbprint $certificateThumbprint `
    -Path artifacts/estel-portable-x86_64.exe, artifacts/Estel-Setup-x86_64.exe `
    -ChecksumPath artifacts/SHA256SUMS.txt
```

O SignTool utiliza `/fd SHA256` para a assinatura e `/tr ... /td SHA256` para carimbo RFC 3161; `/td` vem depois de `/tr`. O verificador executa `signtool verify /pa /all /tw` com prazo de 30 segundos por arquivo, exige assinatura incorporada válida, editor esperado e carimbo de tempo, rejeita autoassinatura e gera os hashes somente depois de todos os arquivos passarem. Arquivos permanecem bloqueados para escrita durante sua verificação e cálculo de hash. Se a execução falhar, um manifesto anterior pode continuar presente: interrompa a publicação. [Referência do SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool).

Instale o pacote em uma máquina de teste com Smart App Control ativo e verifique também o `unins000.exe` instalado com o mesmo script e certificado esperado. Execute o aplicativo, sua janela de configurações, os auxiliares e a desinstalação. A Microsoft recomenda assinatura em todos os componentes executáveis, inclusive desinstaladores e arquivos temporários executados. Validação de confiança nesta máquina e assinatura válida não garantem a decisão final de reputação em outro computador. [Requisitos de controle de aplicativos](https://learn.microsoft.com/en-us/windows/security/book/application-security-application-and-driver-control).

## Compilação e validação disponíveis

Em 02/10/2026, a distribuição oficial Rust 1.96.0 foi baixada por HTTPS, conferida contra o manifesto e seus SHA-256 publicados e executou `rustc -vV` e `cargo --version`. O `cargo check --all-targets --locked` local foi bloqueado pelo Code Integrity ao carregar uma DLL de macro procedural recém-compilada. Isso ocorre antes de produzir o aplicativo e não é resolvido apenas assinando seu instalador. A política permaneceu ativa; não há recomendação de desativar SAC, mudar políticas ou variar binários para escapar da decisão. A verificação em CI é uma rota separada e seu resultado deve ser conferido antes de publicar. [Registro sanitizado](../artifacts/performance/rust_196_code_integrity.json).

O script de verificação foi exercitado com um binário do SDK já assinado pela Microsoft e com rejeições de arquivo sem assinatura, editor diferente, caminho ausente e destino de checksum igual ao arquivo verificado. Isso valida o mecanismo de conferência, não uma assinatura do Estel. A assinatura e a instalação do pacote assinado ainda dependem da contratação do certificado/serviço e da execução completa do procedimento acima.

Os sete testes passaram em 02/10/2026; o lote com um arquivo inválido também preservou o manifesto anterior. [Resultado sanitizado](../artifacts/performance/signature-tests.json). Para reproduzir sem assinar ou executar o Estel:

```powershell
./scripts/Test-EstelSignatures.Tests.ps1 -SignToolPath $signTool `
    -UnsignedExecutable 'CAMINHO_DE_UM_ESTEL_SEM_ASSINATURA.exe'
```
