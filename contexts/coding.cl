# Coding workflow - Focused on code generation
name = coding

[understand]
task_template = tasks/plan
role_template = roles/senior_dev
transition -> code on next

[code]
task_template = tasks/code
transition -> test on next
auto_rule: turn > 10 -> next

[test]
task_template = tasks/test
transition -> review on next

[review]
task_template = tasks/review
transition -> done on done

[done]
task_template = tasks/done
