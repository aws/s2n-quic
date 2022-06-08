package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;

public class ClientStack extends Stack {
    public ClientStack(final Construct parent, final String id) {
        this(parent, id, null, null);
    }

    public ClientStack(final Construct parent, final String id, Bucket metricsBucket,
        final StackProps props) {
        super(parent, id, props);
        String bucketName = metricsBucket.getBucketArn();
    }
}                                         

