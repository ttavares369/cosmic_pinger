# 1. Cria a pasta de binários do usuário (se não existir)
mkdir -p ~/bin

# 2. Copia o executável compilado para lá
cp target/release/cosmic_pinger ~/bin/

# 3. Cria o arquivo de Auto-Start
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