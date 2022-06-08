package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.StackProps;

import java.lang.IllegalArgumentException;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;

public class NetbenchAutoApp {

    private static Set<String> awsRegions;

    // Helper method to build an environment
    static Environment makeEnv(String account, String region) {
        return Environment.builder()
            .account(account)
            .region(region)
            .build();
    }

    public static void main(final String[] args) {
        App app = new App();
        awsRegions = new HashSet<>(
            Arrays.asList("us-east-1", "us-east-2", "us-west-1",
                "us-west-2", "af-south-1", "ap-east-1", "ap-southeast-3", "ap-south-1",
                "ap-northeast-3", "ap-northeast-2", "ap-southeast-1", "ap-southeast-2",
                "ap-northeast-1", "ca-central-1", "eu-central-1", "eu-west-1", "eu-west-2",
                "eu-south-1", "eu-west-3", "eu-north-1", "me-south-1", "sa-east-1", 
                "us-gov-east-1", "us-gov-west-1"
            )
        );

        // Context variable default values and validation
        String protocol = (String)app.getNode().tryGetContext("protocol");
        protocol = (protocol == null) ? "s2n-quic" : protocol.toLowerCase();

        if (!protocol.equals("s2n-quic")) 
            throw new IllegalArgumentException("Invalid protocol, only s2n-quic is currently supported.");

        String awsAccount = (String)app.getNode().tryGetContext("aws-account");
        awsAccount = (awsAccount == null) 
            ? System.getenv("CDK_DEFAULT_ACCOUNT") 
            : awsAccount;
        
        String clientRegion = (String)app.getNode().tryGetContext("client-region");
        clientRegion = (clientRegion == null) 
            ? System.getenv("CDK_DEFAULT_REGION") 
            : clientRegion.toLowerCase();

        if (!awsRegions.contains(clientRegion)) 
            throw new IllegalArgumentException("Invalid client region.");

        String serverRegion = (String)app.getNode().tryGetContext("server-region");
        serverRegion = (serverRegion == null) 
            ? System.getenv("CDK_DEFAULT_REGION") 
            : serverRegion.toLowerCase();
            
        if (!awsRegions.contains(serverRegion)) 
            throw new IllegalArgumentException("Invalid server region.");

        String ec2InstanceType = (String)app.getNode().tryGetContext("instance-type");
        ec2InstanceType = (ec2InstanceType == null) 
            ? "c5n.xlarge" 
            : ec2InstanceType.toLowerCase();
        
        // Stack instantiation
        ReportStack reportStack = new ReportStack(app, "ReportStack", StackProps.builder()
            .build());

        ClientStack clientStack = new ClientStack(app, "ClientStack", reportStack.getBucket(),
            StackProps.builder()
                .env(makeEnv(awsAccount, clientRegion))
                .build());

        ServerStack serverStack = new ServerStack(app, "ServerStack", StackProps.builder()
            .env(makeEnv(awsAccount, serverRegion))
            .build());

        StateMachineStack stateMachineStack = new StateMachineStack(app, "StateMachineStack", StackProps.builder()
            .build());

        app.synth();
    }
}

