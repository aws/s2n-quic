# duvet-action

This action builds and installs [Duvet](https://github.com/aws/s2n-quic/tree/main/common/duvet), generates a compliance report via a provided script, and publishes the result to an S3 bucket.

# Usage

### `report-script: ''`

Path to a script that generates a Duvet report. See `duvet report --help` for more information about generating reports. The action expects the report to be generated to `report-path`.

The script will be passed `github.sha` in the first argument, which can be used with the `--blob-link` Duvet argument.

### `report-path: '''`

Path to the output report generated in `report-script`. Defaults to `report.html` in the same directory that `report-script` is in.

### `aws-access-key-id: ''`

An AWS access key. The corresponding user must have S3 write permissions.

### `aws-secret-access-key: ''`

The AWS secret key.

### `aws-s3-bucket-name: ''`

The name of the destination S3 bucket which the report will be uploaded to.

### `cdn: ''`

An optional CDN which will prefix the published S3 URL in the `compliance / report` Github check.

### `s2n-quic-dir: ''`

Path to the directory where s2n-quic is cloned. Used for repositories other than s2n-quic, which first clone s2n-quic to call this action.


## Example usage:

```yml
jobs:
  duvet:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/checkout@v3
        with:
          repository: aws/s2n-quic
          path: ./s2n-quic
          submodules: true
      - uses: ./s2n-quic/.github/actions/duvet
        with:
          s2n-quic-dir: ./s2n-quic
          report-script: compliance/generate_report.sh
          aws-access-key-id: ${{ secrets.AWS_ACCESS_KEY_ID }}
          aws-secret-access-key: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
          aws-s3-bucket-name: s2n-tls-ci-artifacts
          aws-s3-region: us-west-2
          cdn: https://d3fqnyekunr9xg.cloudfront.net
```
