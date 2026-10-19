# Perfil do jogo (`perfil.toml`)

Quando a extração automática não acha o texto de um jogo, quem estudou o
arquivo — uma pessoa ou uma IA — descreve onde e como ele está num arquivo
pequeno, `perfil.toml`, na pasta do projeto. Com ele o app lê as frases pelos
ponteiros, traduz, valida e regrava, sem crescer o arquivo.

O perfil não tem byte do jogo: só posições, códigos e a tabela de letras.

## O que ele descreve

```toml
[[texto]]
nome = "overlay 0"          # aparece como o arquivo de cada frase
tabela = "letras"           # tables/letras.tbl no projeto
inicio = 0x120000           # faixa do arquivo onde as frases moram
fim = 0x138000
cabecalho = "0100"          # bytes que abrem cada frase (pode ser vazio)
terminador = "EA"           # bytes que fecham cada frase
alinhamento = 4             # cada frase começa num múltiplo disto
quebra = "E6"               # código que quebra a linha (opcional)
pagina = "E5"               # código que fecha uma página da caixa (opcional)
parametros = { E1 = 2, E2 = 1, E3 = 1, E7 = 1 }   # código = bytes de parâmetro

[texto.ponteiros]
memoria = 0x02180000        # endereço de memória onde o trecho é carregado...
origem = 0xA0000            # ...e a posição dele no arquivo
procurar = [[0x4000, 0x90000], [0xA0000, 0x140000]]   # onde procurar ponteiros
```

Ponteiro = palavra de 32 bits, little-endian, alinhada, com o valor
`memoria + (posição da frase no arquivo − origem)`. Num cartucho de GBA,
`memoria = 0x08000000` e `origem = 0`. Num módulo de DS (ARM9 ou overlay),
`memoria` é o endereço de carga e `origem` a posição do módulo no cartucho.

## Rascunho automático

```bash
romtranslate perfil-sugerir jogo.rtsproj --palavra Soma
```

Com uma palavra que aparece na tela (só letras, com maiúscula e minúscula,
como o nome do herói), o app propõe tudo de uma vez e escreve um
`perfil.toml` de rascunho (ou `perfil-rascunho.toml`, se o projeto já tem
um — o que existe nunca é tocado):

- a tabela de letras pela busca relativa (`tables/perfil-letras.tbl`; se
  maiúscula e minúscula estão a 0x20 uma da outra, o ASCII imprimível
  deslocado inteiro);
- os ponteiros de 32 bits pras frases em volta da palavra, nas bases comuns:
  cartucho de GBA em `0x08000000`, e no DS cada módulo (ARM9, ARM7, overlay)
  pelo endereço de carga do header — o `memoria`, a `origem` e o `procurar`
  saem daí;
- o cabeçalho (o começo mais comum das frases com ponteiro), o terminador (o
  último byte antes da frase seguinte) e o alinhamento;
- a faixa, lendo o jogo com o rascunho em volta da palavra.

Ele diz quantas frases o rascunho lê, mostra as que têm a palavra e lista os
códigos fora da tabela com quantas vezes aparecem. Quebra de linha, fim de
página e códigos com parâmetro ficam em comentário pra conferir: o mais
comum costuma ser a quebra e o segundo a página, mas só o jogo confirma. No
jogo de DS de 19/10 o rascunho achou o mesmo módulo, endereço, cabeçalho,
terminador e alinhamento do perfil escrito à mão, e leu 1.214 das 1.224
frases (as outras precisavam dos parâmetros declarados).

## Como achar cada valor

1. **Tabela de letras:** procure uma palavra que aparece na tela (nome do
   herói, de um item) com a busca relativa do app ou à mão. Se o texto é
   ASCII deslocado, a tabela sai direto; letras acentuadas: desenhe a fonte do
   jogo e confira cada código.
2. **Cabeçalho, terminador e alinhamento:** olhe os bytes em volta de três ou
   quatro frases conhecidas.
3. **Códigos com parâmetro:** um código seguido sempre de um byte que não é
   letra leva parâmetro. Declare-o: sem isso, o parâmetro vira letra e a IA
   pode trocá-lo.
4. **Ponteiros:** calcule o endereço de memória de uma frase e procure esse
   valor de 32 bits no arquivo. Onde ele aparece é a tabela de ponteiros;
   `procurar` cobre os trechos que o jogo carrega junto.
5. **Faixa:** do primeiro ao último cabeçalho com ponteiro.

Depois: `romtranslate extract`. A saída diz quantas frases entraram e
quantos candidatos foram descartados (dado que abre igual a uma frase). A
tabela ganha, no fim, uma seção gerada com cada código achado
(`E310=[E3:10]`); não edite essa seção.

## Como a gravação funciona

- A frase que cabe no espaço dela fica no mesmo lugar. Só a que cresceu muda,
  para a sobra (o resto das que encolheram e o lugar das cópias repetidas),
  e os ponteiros dela são reapontados.
- O arquivo nunca cresce: trecho carregado na memória costuma ter outro dado
  logo depois.
- Cada frase tem um teto de bytes (`max_bytes`), que vai para a IA como
  limite de caracteres. Frase curta (nome) ganha a sobra que as repetidas
  liberam.
- Com `quebra`, a linha que passa da largura medida no jogo é quebrada no
  espaço. Com `pagina`, cada página da caixa não pode ter mais linhas que no
  original: o pedido à IA leva quantas linhas cabem em cada página
  (`maxLines`), o que passaria vira erro de validação, e a frase fica no
  original até ser encurtada.
- Toda frase é conferida: cada ponteiro leva aos bytes novos dela, senão nada
  é gravado.
