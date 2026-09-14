# Política de Segurança

Este projeto parseia arquivos binários não confiáveis — bugs de parser são bugs de
segurança.

## Reporte

Abra um **security advisory** privado no GitHub (Security → Report a vulnerability)
para:

- crash/panic ou leitura fora de limites em parsers (headers, futuras tabelas de
  ponteiros, codecs de compressão);
- path traversal em extração de containers;
- decompression bombs / consumo descontrolado de memória;
- exposição de API keys ou secrets em logs, arquivos de projeto ou exports.

Não abra issue pública antes de correção disponível. Bugs comuns (UI, detecção
errada sem crash) podem ir em issues normais.

## Princípios do projeto

- Secrets ficam no secret storage do SO, nunca em config versionável nem em logs;
- Todo acesso a bytes valida limites antes do slice;
- Escrita binária só em cópia de trabalho, nunca no arquivo original.
