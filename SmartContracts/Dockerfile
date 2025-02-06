FROM node:22

WORKDIR /app

COPY . .

RUN apt-get update && apt-get install -y jq

RUN git init && \
    curl -L https://foundry.paradigm.xyz | bash && \
    . ~/.bashrc && \
    foundryup && \
    forge install && \
    git submodule update --init --recursive

ENV PATH="/root/.foundry/bin:$PATH"

ENTRYPOINT ["sh", "-c"]

CMD ["echo Please verify the environment variables and command."]