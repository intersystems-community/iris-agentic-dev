# TARGETARCH is set by docker buildx: "amd64" or "arm64"
ARG TARGETARCH=amd64
FROM gcr.io/distroless/static-debian12
COPY bin/iris-agentic-dev-${TARGETARCH} /iris-agentic-dev
ENTRYPOINT ["/iris-agentic-dev"]
