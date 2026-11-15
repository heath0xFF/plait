use crate::model::{Event, Machine, Project, Thread, ThreadStatus};
use std::sync::Arc;

fn conversation(topic: &'static str) -> Arc<Vec<Event>> {
    Arc::new(vec![
        Event {
            author: "You",
            time: "10:42",
            body: topic,
            tool: false,
        },
        Event {
            author: "Agent",
            time: "10:42",
            body: "I’ll start with the existing boundaries and keep the change small. First, I’ll inspect the project structure and identify the checks that cover this path.",
            tool: false,
        },
        Event {
            author: "Read · project context",
            time: "10:43",
            body: "README.md\nProject → Machine → Thread\nNative desktop · independent machines · read-only Git in M1",
            tool: true,
        },
        Event {
            author: "Agent",
            time: "10:43",
            body: "The important boundary is the checkout. Conversations belong to a machine, and Git state belongs to that machine’s working directory. Switching threads must not silently switch or modify branches.",
            tool: false,
        },
        Event {
            author: "Plan · 3 steps",
            time: "10:44",
            body: "1. Keep the core models independent of the UI.\n2. Read repository state outside the UI thread.\n3. Test selection changes and repository edge cases.",
            tool: true,
        },
        Event {
            author: "Agent",
            time: "10:44",
            body: "This is an example conversation, not an agent run. You can inspect real local changes in the Changes view. Nothing in this transcript has executed a command or edited a file.",
            tool: false,
        },
    ])
}

pub fn projects() -> Vec<Project> {
    vec![
        Project {
            name: "Plait".into(),
            machines: vec![
                Machine {
                    name: "Zaphod",
                    local: true,
                    threads: vec![
                        Thread {
                            title: "Native desktop foundation",
                            status: ThreadStatus::Ready,
                            events: conversation(
                                "Let’s build a fast native foundation for Plait. Keep the conversation central, and make local Git changes easy to inspect.",
                            ),
                        },
                        Thread {
                            title: "Review Git boundaries",
                            status: ThreadStatus::NeedsInput,
                            events: conversation(
                                "Review how read-only Git fits into the desktop app. What should happen when a checkout changes during a refresh?",
                            ),
                        },
                    ],
                },
                Machine {
                    name: "Marvin",
                    local: false,
                    threads: vec![Thread {
                        title: "Repository discovery",
                        status: ThreadStatus::Working,
                        events: conversation(
                            "Outline how a project could be discovered on Marvin. Do not connect to the machine or start a process.",
                        ),
                    }],
                },
                Machine {
                    name: "Arthur",
                    local: false,
                    threads: vec![Thread {
                        title: "Host recovery notes",
                        status: ThreadStatus::Complete,
                        events: conversation(
                            "Document the questions we need to answer before persistent remote threads can run on Arthur.",
                        ),
                    }],
                },
            ],
        },
        Project {
            name: "Field notes".into(),
            machines: vec![Machine {
                name: "Arthur",
                local: false,
                threads: vec![Thread {
                    title: "A fresh conversation",
                    status: ThreadStatus::Ready,
                    events: Arc::new(vec![]),
                }],
            }],
        },
    ]
}
