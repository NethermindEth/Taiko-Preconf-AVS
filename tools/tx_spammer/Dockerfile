# Use an official Python runtime as a parent image
FROM python:3.9-slim

# Set the working directory inside the container
WORKDIR /app

# Copy the current directory contents into the container
COPY . /app

# Install any necessary dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy the script to the working directory
COPY tx_spammer.py /app/tx_spammer.py

# Command to run the Python script with the arguments based on environment variables
CMD python -u tx_spammer.py \
    --count "$TX_COUNT" \
    --amount "$TX_AMOUNT" \
    --rpc "$RPC_URL" \
    ${SLOTS:+--slots "$SLOTS"} \
    --beacon-rpc "$BEACON_RPC_URL" \
    --sleep "$SLEEP" \
    --batch-size "$BATCH_SIZE"