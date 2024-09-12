# Build the Docker image
docker build -t mev-boost-mock .

# Run the Docker container
docker run -d -p 8080:8080 --name mev-boost-mock mev-boost-mock