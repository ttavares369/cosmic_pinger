# Cosmic Pinger

Aplicativo em Rust criado para o Pop!_OS Cosmic 24.04 LTS. Ele monitora múltiplos sites/hosts via `ping`, consolida os resultados e mostra um indicador colorido na bandeja do sistema (KSNI). Quando todos os destinos respondem, o ícone fica verde; em caso de falha o ícone fica vermelho, e durante a inicialização ele aparece amarelo.

## Funcionalidades
- Monitoramento cíclico com atualizações a cada 3 minutos.
- Configuração gráfica (`--config`) para adicionar/remover URLs sem editar arquivos manualmente.
- Persistência automática da lista em `~/.config/com/tiago/cosmic_pinger/sites.json`.
- Menu da bandeja com status individuais, última atualização e atalho para encerrar.
- Compatível com Pop!_OS Cosmic/Wayland mantendo footprint leve (binário único).

<img width="782" height="546" alt="image" src="https://github.com/user-attachments/assets/d17bf70f-db6d-4ef4-933f-9a8dd5db59b2" />


## Requisitos
- Rust 1.76+ (toolchain stable).
- Dependências do sistema necessárias para compilar aplicativos Iced/Ksni (no Pop!_OS já estão presentes por padrão).

## Build
```bash
cargo build --release
```
O binário ficará em `target/release/cosmic_pinger`.

## Configuração
Execute o modo gráfico para gerenciar os destinos monitorados:
```bash
./target/release/cosmic_pinger --config
```
As entradas são salvas em `~/.config/com/tiago/cosmic_pinger/sites.json`. Você também pode editar esse arquivo manualmente se preferir.

## Execução
```bash
./target/release/cosmic_pinger
```
O serviço sobe em modo bandeja; acompanhe os logs no terminal se quiser ver o output do ciclo de monitoramento.

## Auto-start no COSMIC
1. Crie a pasta de binários do usuário (se não existir)
	```bash
	mkdir -p ~/bin
	```
2. Copie o executável compilado para lá
	```bash
	cp target/release/cosmic_pinger ~/bin/
	```
3. Crie o arquivo de auto-start
	```bash
	cat <<EOF > ~/.config/autostart/cosmic_pinger.desktop
	[Desktop Entry]
	Type=Application
	Exec=$HOME/bin/cosmic_pinger
	Hidden=false
	NoDisplay=false
	X-GNOME-Autostart-enabled=true
	Name=Cosmic Pinger
	Comment=Monitoramento de Servidores
	Icon=network-transmit-receive
	EOF
	```

Após o próximo login no Pop!_OS Cosmic, o app carregará automaticamente e o indicador aparecerá na bandeja.
