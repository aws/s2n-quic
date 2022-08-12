// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.regioninfo.Fact;
import java.lang.IllegalArgumentException;
import java.util.HashSet;
import java.util.Set;

public class NetbenchAutoApp {

    // Helper method to build an environment
    static Environment makeEnv(String account, String region) {
        return Environment.builder()
            .account(account)
            .region(region)
            .build();
    }

    public static void main(final String[] args) {
        App app = new App();
        Set<String> awsRegions = new HashSet<>(Fact.getRegions());

        // Context variable default values and validation
        String protocol = (String)app.getNode().tryGetContext("protocol");
        protocol = (protocol == null) ? "s2n-quic" : protocol.toLowerCase();

        if (!protocol.equals("s2n-quic")) {
            throw new IllegalArgumentException("Invalid protocol, only s2n-quic is currently supported.");
        }
            
        String awsAccount = (String)app.getNode().tryGetContext("aws-account");
        awsAccount = (awsAccount == null) 
            ? System.getenv("CDK_DEFAULT_ACCOUNT") 
            : awsAccount;
        
        String clientRegion = (String)app.getNode().tryGetContext("client-region");
        clientRegion = (clientRegion == null) 
            ? System.getenv("CDK_DEFAULT_REGION") 
            : clientRegion.toLowerCase();

        if (!awsRegions.contains(clientRegion)) {
            throw new IllegalArgumentException("Invalid client region.");
        }

        String serverRegion = (String)app.getNode().tryGetContext("server-region");
        serverRegion = (serverRegion == null) 
            ? System.getenv("CDK_DEFAULT_REGION") 
            : serverRegion.toLowerCase();
            
        if (!awsRegions.contains(serverRegion)) {
            throw new IllegalArgumentException("Invalid server region.");
        }

        String ec2InstanceType = (String)app.getNode().tryGetContext("instance-type");
        ec2InstanceType = (ec2InstanceType == null) 
            ? "t4g.xlarge" 
            : ec2InstanceType.toLowerCase();

        String serverEcrUri = (String)app.getNode().tryGetContext("server-ecr-uri");
        serverEcrUri = (serverEcrUri == null) 
            ? "public.ecr.aws/d2r9y8c2/s2n-quic-collector-server-scenario:latest" 
            : serverEcrUri;

        String clientEcrUri = (String)app.getNode().tryGetContext("client-ecr-uri");
        clientEcrUri = (clientEcrUri == null) 
            ? "public.ecr.aws/d2r9y8c2/s2n-quic-collector-client-scenario:latest"
            : clientEcrUri;

        String scenarioFile = (String)app.getNode().tryGetContext("scenario");
        scenarioFile = (scenarioFile == null) 
            ? "/usr/bin/request_response.json"
            : scenarioFile;

        String arm = (String)app.getNode().tryGetContext("arm");
        arm = (arm == null) 
            ? "true"
            : arm.toLowerCase();

        if (!arm.equals("true") && !arm.equals("false")) {
            throw new IllegalArgumentException("arm must be true or false.");
        }
    
        // Stack instantiation   
        VpcStack vpcStack = new VpcStack(app, "VpcStack", VpcStackProps.builder()
            .env(makeEnv(awsAccount, serverRegion))
            .cidr("11.0.0.0/16")
            .build());

        EcsStack serverEcsStack = new EcsStack(app, "ServerEcsStack", EcsStackProps.builder()
            .env(makeEnv(awsAccount, serverRegion))
            .bucket(vpcStack.getBucket())
            .stackType("server")
            .vpc(vpcStack.getVpc())
            .instanceType(ec2InstanceType)
            .ecrUri(serverEcrUri)
            .scenario(scenarioFile)
            .serverRegion(serverRegion)
            .arm(arm)
            .build());

        serverEcsStack.addDependency(vpcStack);

        EcsStack clientEcsStack = new EcsStack(app, "ClientEcsStack", EcsStackProps.builder()
            .env(makeEnv(awsAccount, clientRegion))
            .bucket(vpcStack.getBucket())
            .stackType("client")
            .vpc(vpcStack.getVpc())
            .instanceType(ec2InstanceType)
            .serverRegion(serverRegion)
            .dnsAddress(serverEcsStack.getDnsAddress())
            .ecrUri(clientEcrUri)
            .scenario(scenarioFile)
            .arm(arm)
            .build());
        
        clientEcsStack.addDependency(serverEcsStack);

        StateMachineStack stateMachineStack = new StateMachineStack(app, "StateMachineStack", StateMachineStackProps.builder()
            .env(makeEnv(awsAccount, clientRegion))
            .clientTask(clientEcsStack.getEcsTask())
            .bucket(vpcStack.getBucket())
            .logsLambda(serverEcsStack.getLogsLambda())
            .cluster(clientEcsStack.getCluster())
            .protocol(protocol)
            .build());

        stateMachineStack.addDependency(clientEcsStack);

        app.synth();
    }
}

