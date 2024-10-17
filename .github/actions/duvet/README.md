# duvet-action

This action builds and installs [Duvet](https://github.com/aws/s2n-quic/tree/main/common/duvet), generates a compliance report via a provided script, and publishes the result to an S3 bucket.

# Usage

### `report-script: ''`

Path to a script that generates a Duvet report. See `duvet report --help` for more information about generating reports. The action expects the report to be generated to `report-path`.

The script will be passed `github.sha` in the first argument, which can be used with the `--blob-link` Duvet argument.

### `report-path: '''`

Path to the output report generated in `report-script`. Defaults to `report.html` in the same directory that `report-script` is in.

### `aws-access-key-id: ''`

Deprecated.  This was previously used to authenticate with long lived IAM credentials. See [Configuring OpenID Connect](https://docs.github.com/en/actions/security-for-github-actions/security-hardening-your-deployments/configuring-openid-connect-in-cloud-providers)

### `aws-secret-access-key: ''`

Deprecated.  This was previously used to authenticate with long lived IAM credentials. See [Configuring OpenID Connect](https://docs.github.com/en/actions/security-for-github-actions/security-hardening-your-deployments/configuring-openid-connect-in-cloud-providers)

### `role-to-assume: ''`

For Open ID Connect, the role attached to the IdP, in the form of an ARN. Intended for use with [AWS](https://docs.github.com/en/actions/security-for-github-actions/security-hardening-your-deployments/configuring-openid-connect-in-amazon-web-services)

### `role-session-name: ''`

For Open ID Connect, an arbitrary session name. Intended for use with [AWS](https://docs.github.com/en/actions/security-for-github-actions/security-hardening-your-deployments/configuring-openid-connect-in-amazon-web-services)

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
          role-to-assume: arn:aws:iam::123456789:role/GitHubOIDCRole
          role-session-name: GithubActionSession
          aws-s3-bucket-name: s2n-tls-ci-artifacts
          aws-s3-region: us-west-2
          cdn: https://d3fqnyekunr9xg.cloudfront.net
```
