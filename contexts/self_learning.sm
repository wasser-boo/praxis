# Self-learning workflow - Learn from interactions
name = self_learning

[observe]
task_template = tasks/plan
role_template = roles/researcher
transition -> learn on next

[learn]
task_template = tasks/code
transition -> apply on next

[apply]
task_template = tasks/test
transition -> reflect on next

[reflect]
task_template = tasks/review
transition -> done on done
auto_rule: turn > 5 -> done

[done]
task_template = tasks/done
