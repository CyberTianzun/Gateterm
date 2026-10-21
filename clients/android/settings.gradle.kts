pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        maven {
            url = uri("https://maven.aliyun.com/nexus/content/repositories/releases/")
            content { includeGroup("com.aliyun.ams") }
        }
        google()
        mavenCentral()
    }
}

rootProject.name = "octoterm-android"
include(":app")
