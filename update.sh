#!/bin/bash
echo "Puxando e Recompilando..."
git pull && cargo build --release

if [ $? -eq 0 ]; then
    echo "Substituindo executável antigo..."
    cp target/release/cosmic_pinger ~/bin/

    echo "Reiniciando applet..."
    pkill -f cosmic_pinger
    
    # Espera um segundo para garantir que o processo foi encerrado
    sleep 1 
    
    ~/bin/cosmic_pinger &
    
    echo "Pronto! Applet atualizado e rodando."
else
    echo "Erro na compilação. Abortando atualização."
fi