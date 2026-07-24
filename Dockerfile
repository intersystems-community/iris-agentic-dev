FROM gcr.io/distroless/static-debian12
COPY iris-agentic-dev /iris-agentic-dev
ENTRYPOINT ["/iris-agentic-dev"]
